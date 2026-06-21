//! Profile Connection Space (PCS) maths for the colour-management module (ICC-2).
//!
//! The ICC PCS is either CIE XYZ or CIE L*a*b*, both referenced to the **D50**
//! white point. This module provides the two PCS encodings and the conversions
//! a colour-managed pipeline needs:
//!
//! * [`Xyz`] / [`Lab`] tristimulus / perceptual encodings.
//! * [`Xyz::to_lab`] / [`Lab::to_xyz`] — CIE 1976 L*a*b* conversion about a
//!   chosen reference white (round-trips within floating-point ε).
//! * [`Xyz::adapt`] — Bradford chromatic adaptation between two white points,
//!   plus the D50↔D65 helpers the RGB matrix-shaper (ICC-3) relies on (the ICC
//!   PCS is D50; sRGB / Display-P3 / Rec.2020 are authored for D65).
//!
//! Everything is `f64`, allocation-free and panic-free.

use crate::icc::XyzNumber;

/// A CIE 1931 XYZ tristimulus value.
///
/// `y` is luminance, normalised so that a perfect diffuse white has `y == 1.0`.
/// PCS values are referenced to the D50 white point ([`Xyz::D50`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Xyz {
    /// CIE X tristimulus.
    pub x: f64,
    /// CIE Y tristimulus (luminance, `1.0` for reference white).
    pub y: f64,
    /// CIE Z tristimulus.
    pub z: f64,
}

/// A CIE 1976 L*a*b* value.
///
/// `l` is lightness in `[0, 100]`; `a` (green–red) and `b` (blue–yellow) are
/// unbounded opponent axes. A given Lab triple is only meaningful relative to
/// the reference white it was computed against (the ICC PCS uses D50).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Lab {
    /// L* lightness, `[0, 100]`.
    pub l: f64,
    /// a* green(−)→red(+) opponent axis.
    pub a: f64,
    /// b* blue(−)→yellow(+) opponent axis.
    pub b: f64,
}

impl Xyz {
    /// ICC PCS reference white (D50, 2° observer). The ICC profile connection
    /// space is always D50-referenced.
    pub const D50: Xyz = Xyz { x: 0.964_22, y: 1.0, z: 0.825_21 };

    /// sRGB / Display-P3 / Rec.2020 reference white (D65, 2° observer).
    pub const D65: Xyz = Xyz { x: 0.950_47, y: 1.0, z: 1.088_83 };

    /// Constructs an `Xyz` from raw components.
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Xyz { x, y, z }
    }

    /// Converts this XYZ to CIE L*a*b* about the given reference white.
    ///
    /// Uses the CIE 1976 piecewise transfer function; the linear segment near
    /// black keeps the inverse well-behaved.
    pub fn to_lab(self, white: Xyz) -> Lab {
        let fx = lab_f(self.x / white.x);
        let fy = lab_f(self.y / white.y);
        let fz = lab_f(self.z / white.z);
        Lab {
            l: 116.0 * fy - 16.0,
            a: 500.0 * (fx - fy),
            b: 200.0 * (fy - fz),
        }
    }

    /// Bradford chromatic adaptation of this tristimulus from `src_white` to
    /// `dst_white`.
    ///
    /// Cone-response (Bradford) adaptation: convert to LMS-like sharpened cone
    /// space, scale each channel by the destination/source white ratio, convert
    /// back. This is the standard transform for re-referencing a colour from one
    /// adapting white to another (e.g. a D65-authored RGB primary into the D50
    /// PCS, or the reverse).
    pub fn adapt(self, src_white: Xyz, dst_white: Xyz) -> Xyz {
        let m = bradford_adaptation(src_white, dst_white);
        apply_matrix(&m, self)
    }

    /// Adapts a tristimulus referenced to D50 (the ICC PCS) into D65.
    pub fn d50_to_d65(self) -> Xyz {
        self.adapt(Xyz::D50, Xyz::D65)
    }

    /// Adapts a tristimulus referenced to D65 into D50 (the ICC PCS).
    pub fn d65_to_d50(self) -> Xyz {
        self.adapt(Xyz::D65, Xyz::D50)
    }
}

impl Lab {
    /// Constructs a `Lab` from raw components.
    pub fn new(l: f64, a: f64, b: f64) -> Self {
        Lab { l, a, b }
    }

    /// Converts this L*a*b* back to CIE XYZ about the given reference white.
    ///
    /// Exact inverse of [`Xyz::to_lab`] (round-trips within floating-point ε).
    pub fn to_xyz(self, white: Xyz) -> Xyz {
        let fy = (self.l + 16.0) / 116.0;
        let fx = fy + self.a / 500.0;
        let fz = fy - self.b / 200.0;
        Xyz {
            x: white.x * lab_f_inv(fx),
            y: white.y * lab_f_inv(fy),
            z: white.z * lab_f_inv(fz),
        }
    }
}

impl From<XyzNumber> for Xyz {
    fn from(n: XyzNumber) -> Self {
        Xyz { x: n.x, y: n.y, z: n.z }
    }
}

impl From<Xyz> for XyzNumber {
    fn from(c: Xyz) -> Self {
        XyzNumber { x: c.x, y: c.y, z: c.z }
    }
}

/// CIE L*a*b* forward nonlinearity (`f`), with the linear segment below the
/// `(6/29)³` threshold.
fn lab_f(t: f64) -> f64 {
    const DELTA: f64 = 6.0 / 29.0;
    if t > DELTA * DELTA * DELTA {
        t.cbrt()
    } else {
        t / (3.0 * DELTA * DELTA) + 4.0 / 29.0
    }
}

/// Inverse of [`lab_f`].
fn lab_f_inv(t: f64) -> f64 {
    const DELTA: f64 = 6.0 / 29.0;
    if t > DELTA {
        t * t * t
    } else {
        3.0 * DELTA * DELTA * (t - 4.0 / 29.0)
    }
}

/// Bradford cone-response matrix (XYZ → sharpened LMS).
#[rustfmt::skip]
const BRADFORD: [[f64; 3]; 3] = [
    [ 0.895_1,  0.266_4, -0.161_4],
    [-0.750_2,  1.713_5,  0.036_7],
    [ 0.038_9, -0.068_5,  1.029_6],
];

/// Inverse Bradford matrix (sharpened LMS → XYZ).
#[rustfmt::skip]
const BRADFORD_INV: [[f64; 3]; 3] = [
    [ 0.986_992_9, -0.147_054_3,  0.159_962_7],
    [ 0.432_305_3,  0.518_360_3,  0.049_291_2],
    [-0.008_528_7,  0.040_042_8,  0.968_486_7],
];

/// Builds the 3×3 Bradford adaptation matrix that re-references a tristimulus
/// from `src_white` to `dst_white`.
fn bradford_adaptation(src_white: Xyz, dst_white: Xyz) -> [[f64; 3]; 3] {
    let (ls, ms, ss) = mul3(&BRADFORD, src_white);
    let (ld, md, sd) = mul3(&BRADFORD, dst_white);
    // Diagonal cone-ratio scaling D = diag(ld/ls, md/ms, sd/ss).
    let diag = [ld / ls, md / ms, sd / ss];
    // Adapt = BRADFORD_INV * D * BRADFORD.
    let mut scaled = BRADFORD;
    for (row, k) in scaled.iter_mut().zip(diag.iter()) {
        for v in row.iter_mut() {
            *v *= *k;
        }
    }
    matmul(&BRADFORD_INV, &scaled)
}

/// `M * (x, y, z)` returning a tuple.
fn mul3(m: &[[f64; 3]; 3], v: Xyz) -> (f64, f64, f64) {
    (
        m[0][0] * v.x + m[0][1] * v.y + m[0][2] * v.z,
        m[1][0] * v.x + m[1][1] * v.y + m[1][2] * v.z,
        m[2][0] * v.x + m[2][1] * v.y + m[2][2] * v.z,
    )
}

/// `M * (x, y, z)` returning an [`Xyz`].
fn apply_matrix(m: &[[f64; 3]; 3], v: Xyz) -> Xyz {
    let (x, y, z) = mul3(m, v);
    Xyz { x, y, z }
}

/// 3×3 matrix product `a * b`.
fn matmul(a: &[[f64; 3]; 3], b: &[[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let mut out = [[0.0; 3]; 3];
    for (i, out_row) in out.iter_mut().enumerate() {
        for (j, out_cell) in out_row.iter_mut().enumerate() {
            *out_cell = a[i][0] * b[0][j] + a[i][1] * b[1][j] + a[i][2] * b[2][j];
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two `Xyz` agree to within `eps` per component.
    fn xyz_close(a: Xyz, b: Xyz, eps: f64) -> bool {
        (a.x - b.x).abs() < eps && (a.y - b.y).abs() < eps && (a.z - b.z).abs() < eps
    }

    #[test]
    fn lab_round_trips_under_d50() {
        // A spread of tristimuli, all referenced to the D50 PCS white.
        let samples = [
            Xyz::new(0.2, 0.18, 0.1),
            Xyz::new(0.5, 0.5, 0.5),
            Xyz::new(0.9642, 1.0, 0.8252), // ~white
            Xyz::new(0.01, 0.01, 0.01),    // near black, exercises linear segment
            Xyz::new(0.4, 0.2, 0.7),
        ];
        for c in samples {
            let lab = c.to_lab(Xyz::D50);
            let back = lab.to_xyz(Xyz::D50);
            assert!(xyz_close(c, back, 1e-9), "round-trip failed for {c:?} → {lab:?} → {back:?}");
        }
    }

    #[test]
    fn reference_white_maps_to_l100() {
        // The reference white must encode as L*=100, a*=b*=0.
        let lab = Xyz::D50.to_lab(Xyz::D50);
        assert!((lab.l - 100.0).abs() < 1e-9);
        assert!(lab.a.abs() < 1e-9);
        assert!(lab.b.abs() < 1e-9);
    }

    #[test]
    fn known_lab_value() {
        // sRGB mid-grey (linear Y≈0.2, neutral) about D50 → roughly L*≈51.8.
        let lab = Xyz::new(0.2064, 0.2140, 0.1771).to_lab(Xyz::D50);
        assert!((lab.l - 53.28).abs() < 0.5, "L*={}", lab.l);
        assert!(lab.a.abs() < 1.5, "a*={}", lab.a);
        assert!(lab.b.abs() < 1.5, "b*={}", lab.b);
    }

    #[test]
    fn bradford_white_maps_to_white() {
        // Adapting the source white to a destination white must yield exactly
        // the destination white (by construction of the cone-ratio scaling).
        let adapted = Xyz::D65.adapt(Xyz::D65, Xyz::D50);
        assert!(xyz_close(adapted, Xyz::D50, 1e-6), "{adapted:?}");
    }

    #[test]
    fn bradford_d50_d65_round_trip() {
        let c = Xyz::new(0.3, 0.25, 0.4);
        let there = c.d50_to_d65();
        let back = there.d65_to_d50();
        // BRADFORD_INV is a 7-digit rounded constant, not the exact algebraic
        // inverse of BRADFORD, so the adapt→unadapt cycle carries ~1e-7 residual.
        assert!(xyz_close(c, back, 1e-6), "{c:?} → {there:?} → {back:?}");
    }

    #[test]
    fn bradford_d65_reference_value() {
        // D65 white adapted into the D50 PCS must equal the D50 white.
        let adapted = Xyz::D65.d65_to_d50();
        assert!(xyz_close(adapted, Xyz::D50, 1e-4), "{adapted:?}");
    }

    #[test]
    fn xyznumber_interop() {
        let n = XyzNumber { x: 0.5, y: 0.4, z: 0.3 };
        let c: Xyz = n.into();
        assert_eq!(c, Xyz::new(0.5, 0.4, 0.3));
        let back: XyzNumber = c.into();
        assert_eq!(back, n);
    }
}
