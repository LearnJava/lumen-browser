//! Integration test for the ICC parser on a *real* profile.
//!
//! Synthetic byte buffers passed the unit tests, but per project policy a real
//! file is mandatory — the standard IEC sRGB profile shipped with Windows
//! (freely redistributable). This guards against header-offset and tag-table
//! mistakes that synthetic data can mask.

use lumen_core::icc::{DataColorSpace, ProfileClass, ToneCurve};
use lumen_core::{detect_color_space_from_icc, ColorSpace, IccProfile};

/// The bundled real sRGB profile (`tests/fixtures/sRGB.icc`).
const SRGB_ICC: &[u8] = include_bytes!("fixtures/sRGB.icc");

#[test]
fn parses_real_srgb_header() {
    let p = IccProfile::parse(SRGB_ICC).expect("real sRGB profile must parse");
    // Profile is ICC v2.1.
    assert_eq!(p.version, (2, 1, 0));
    assert_eq!(p.class, ProfileClass::Display); // 'mntr'
    assert_eq!(p.data_color_space, DataColorSpace::Rgb);
    assert_eq!(p.pcs, DataColorSpace::Xyz);
}

#[test]
fn parses_real_srgb_tags() {
    let p = IccProfile::parse(SRGB_ICC).expect("parse");
    // Colorant primaries and white point must be present.
    let r = p.red_xyz.expect("rXYZ");
    let g = p.green_xyz.expect("gXYZ");
    let b = p.blue_xyz.expect("bXYZ");
    let w = p.white_point.expect("wtpt");

    // This IEC sRGB profile stores the unadapted media white point D65 in
    // `wtpt` (~0.9505, 1.0, 1.089) — decoding it correctly validates the
    // s15Fixed16 reader against real data.
    assert!((w.x - 0.9505).abs() < 0.01, "wtpt.x = {}", w.x);
    assert!((w.y - 1.0).abs() < 0.01, "wtpt.y = {}", w.y);
    assert!((w.z - 1.089).abs() < 0.01, "wtpt.z = {}", w.z);

    // Red colorant Y is the smallest, green the largest — sanity on channel order.
    assert!(g.y > r.y && g.y > b.y, "green should be most luminous");

    // TRC curves present (sRGB stores a sampled curve, not a single gamma).
    assert!(p.red_trc.is_some());
    match p.red_trc.unwrap() {
        ToneCurve::Table(t) => assert!(t.len() > 16, "sRGB rTRC is a sampled table"),
        other => panic!("expected sampled rTRC table, got {other:?}"),
    }
}

#[test]
fn classifies_real_srgb_as_srgb() {
    // End-to-end: the public entry point must classify the real profile as sRGB
    // from its primaries, with no string sniffing.
    assert_eq!(detect_color_space_from_icc(SRGB_ICC), ColorSpace::Srgb);
    let p = IccProfile::parse(SRGB_ICC).expect("parse");
    assert_eq!(p.color_space(), ColorSpace::Srgb);
}
