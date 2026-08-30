//! Per-session fingerprint noise for canvas randomization (Brave-style, ADR-007 Layer 4).
//!
//! A canvas fingerprint is the hash of the pixels a page reads back after drawing a
//! fixed scene: the same bytes on every visit make a stable identifier, and the tiny
//! per-device differences in rasterization make it a *distinguishing* one. The defence
//! is to perturb what the readback APIs return so the hash differs per session while
//! the picture does not: `getImageData`, `toDataURL`/`toBlob` and `readPixels` answer
//! with noise applied, the buffer the page draws into and the pixels the compositor
//! shows are untouched.
//!
//! Three properties this generator must have, none of which is decoration:
//!
//! 1. **Positional, not sequential.** The perturbation of a pixel is a pure function of
//!    `(seed, x, y, channel)`, so `getImageData(0, 0, w, h)` and `getImageData(x, y, 1, 1)`
//!    report the same value for the same pixel. A stream-shaped generator (one that
//!    advances a state per byte, as this module did until BUG-454) gives a *different*
//!    answer per requested rectangle, which lets a script average the noise away over a
//!    handful of reads — i.e. it costs the picture its exactness and buys nothing.
//! 2. **Bounded to ±1.** The point is to move the hash, not to damage the image. A full
//!    byte of noise (the previous `pixel[0] ^= next_noise_u8()`) turns any readback into
//!    garbage, which breaks every legitimate use — cropping, filters, image processing —
//!    while a one-unit shift is below the threshold of both display and re-encoding.
//! 3. **Transparent black stays transparent black.** A pixel with `alpha == 0` carries no
//!    visual information and the spec defines its RGB as zero; perturbing it would add no
//!    entropy the opaque pixels do not already carry, while breaking the "a fresh canvas
//!    reads back as all zeros" invariant that the platform (and its test suites) rely on.
//!
//! The seed is per session, so a fingerprint gathered today does not match one gathered
//! after a restart. Mixing the document origin into the seed — so two sites in one session
//! cannot compare notes about the same rendered scene — is the caller's job; see
//! `lumen_js::canvas2d`'s document seed.

/// Per-session canvas fingerprint noise generator.
///
/// Holds nothing but the seed: the perturbation of a pixel is derived from the seed and
/// the pixel's own coordinates, never from how many pixels were read before it.
#[derive(Debug, Clone, Copy)]
pub struct CanvasNoiseGenerator {
    seed: u64,
}

/// One round of SplitMix64 — a cheap, well-distributed integer mix.
///
/// Used to fold `(seed, x, y, channel)` into the per-channel perturbation. Any finalizer
/// with good avalanche would do; what matters is that it is a *function* of its input.
fn mix64(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

impl CanvasNoiseGenerator {
    /// Create a generator for the given per-session seed.
    ///
    /// Same seed → same perturbation for the same pixel; different seed → a different
    /// fingerprint for the same drawing.
    pub fn new(seed: u64) -> Self {
        Self { seed }
    }

    /// Perturbation for one colour channel of the pixel at `(x, y)`: `-1`, `0` or `+1`.
    ///
    /// `channel` is `0`/`1`/`2` for R/G/B. Alpha is never perturbed — a change there is
    /// visible through compositing, which would make the noise a rendering defect rather
    /// than a privacy measure.
    pub fn channel_delta(&self, x: u32, y: u32, channel: u8) -> i16 {
        let h = mix64(
            self.seed
                ^ mix64(u64::from(x).wrapping_mul(0x9E37_79B9_7F4A_7C15))
                ^ mix64(u64::from(y).wrapping_mul(0xC2B2_AE3D_27D4_EB4F))
                ^ mix64(u64::from(channel).wrapping_add(1)),
        );
        (h % 3) as i16 - 1
    }

    /// Apply noise in place to the RGBA8 rectangle `out`, whose top-left pixel is at
    /// `(sx, sy)` in the canvas the pixels came from and which is `w × h` pixels.
    ///
    /// Taking the rectangle's canvas-space origin rather than just the buffer is the whole
    /// point of property 1 above: without it the same pixel would be perturbed differently
    /// depending on which rectangle the page asked for.
    pub fn apply_noise_to_rect(&self, out: &mut [u8], sx: i32, sy: i32, w: u32, h: u32) {
        for row in 0..h {
            for col in 0..w {
                let di = ((row as usize) * (w as usize) + col as usize) * 4;
                let Some(px) = out.get_mut(di..di + 4) else {
                    return;
                };
                // Transparent black is left exactly as the spec defines it (property 3).
                if px[3] == 0 {
                    continue;
                }
                // Pixels outside the canvas were reported as transparent black and were
                // skipped above, so the coordinates below are always the real ones.
                let x = (i64::from(sx) + i64::from(col)).rem_euclid(1 << 24) as u32;
                let y = (i64::from(sy) + i64::from(row)).rem_euclid(1 << 24) as u32;
                for (channel, slot) in px[..3].iter_mut().enumerate() {
                    let delta = self.channel_delta(x, y, channel as u8);
                    *slot = (i16::from(*slot) + delta).clamp(0, 255) as u8;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    // `panic!`/`unwrap` — штатный способ провалить тест (docs/lint-policy.md §10).
    #![allow(clippy::panic, clippy::unwrap_used, clippy::indexing_slicing)]
    use super::*;

    /// Opaque 1×1 buffer of the given colour.
    fn px(r: u8, g: u8, b: u8, a: u8) -> Vec<u8> {
        vec![r, g, b, a]
    }

    #[test]
    fn same_seed_same_pixel_same_delta() {
        let a = CanvasNoiseGenerator::new(42);
        let b = CanvasNoiseGenerator::new(42);
        for x in 0..8 {
            for y in 0..8 {
                for c in 0..3 {
                    assert_eq!(a.channel_delta(x, y, c), b.channel_delta(x, y, c));
                }
            }
        }
    }

    #[test]
    fn different_seeds_disagree_somewhere() {
        let a = CanvasNoiseGenerator::new(42);
        let b = CanvasNoiseGenerator::new(43);
        let differs = (0..16)
            .flat_map(|x| (0..16).map(move |y| (x, y)))
            .any(|(x, y)| (0..3).any(|c| a.channel_delta(x, y, c) != b.channel_delta(x, y, c)));
        assert!(differs, "a different session must fingerprint differently");
    }

    #[test]
    fn delta_never_exceeds_one() {
        let g = CanvasNoiseGenerator::new(7);
        for x in 0..64 {
            for y in 0..64 {
                for c in 0..3 {
                    let d = g.channel_delta(x, y, c);
                    assert!((-1..=1).contains(&d), "delta {d} out of ±1 at {x},{y},{c}");
                }
            }
        }
    }

    #[test]
    fn a_pixel_reads_the_same_whichever_rect_asked_for_it() {
        // The property a stream-shaped generator cannot have: the perturbation of the
        // pixel at (3, 2) must not depend on whether the page read the whole canvas or
        // just that one pixel. Averaging the noise away over repeated reads is exactly
        // what this defeats.
        let g = CanvasNoiseGenerator::new(99);
        let mut whole = vec![128u8; 4 * 4 * 4];
        for p in whole.chunks_exact_mut(4) {
            p[3] = 255;
        }
        let mut one = px(128, 128, 128, 255);
        g.apply_noise_to_rect(&mut whole, 0, 0, 4, 4);
        g.apply_noise_to_rect(&mut one, 3, 2, 1, 1);
        let at = (2 * 4 + 3) * 4;
        assert_eq!(&whole[at..at + 4], &one[..]);
    }

    #[test]
    fn alpha_is_never_touched() {
        let g = CanvasNoiseGenerator::new(1);
        let mut buf = px(10, 20, 30, 200);
        g.apply_noise_to_rect(&mut buf, 0, 0, 1, 1);
        assert_eq!(buf[3], 200);
    }

    #[test]
    fn transparent_black_stays_transparent_black() {
        // A fresh canvas must still read back as all zeros (property 3).
        let g = CanvasNoiseGenerator::new(5);
        let mut buf = vec![0u8; 8 * 8 * 4];
        g.apply_noise_to_rect(&mut buf, 0, 0, 8, 8);
        assert!(buf.iter().all(|&b| b == 0), "empty canvas must stay empty");
    }

    #[test]
    fn saturating_at_the_ends_of_the_range() {
        let g = CanvasNoiseGenerator::new(3);
        let mut black = px(0, 0, 0, 255);
        let mut white = px(255, 255, 255, 255);
        g.apply_noise_to_rect(&mut black, 0, 0, 1, 1);
        g.apply_noise_to_rect(&mut white, 0, 0, 1, 1);
        assert!(black[..3].iter().all(|&c| c <= 1));
        assert!(white[..3].iter().all(|&c| c >= 254));
    }

    #[test]
    fn noise_moves_the_hash_of_a_drawn_scene() {
        // The measure only works if a realistic buffer actually changes.
        let g = CanvasNoiseGenerator::new(2026);
        let mut buf: Vec<u8> = (0..32 * 32)
            .flat_map(|i| [(i % 251) as u8, (i % 241) as u8, (i % 239) as u8, 255])
            .collect();
        let before = buf.clone();
        g.apply_noise_to_rect(&mut buf, 0, 0, 32, 32);
        assert_ne!(before, buf, "an opaque scene must be perturbed");
    }
}
