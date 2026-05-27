//! Per-session fingerprint noise for canvas randomization (Brave-style).
//!
//! Anti-detection: `getImageData()` returns RGBA data with per-session deterministic noise
//! applied to pixel channels. The noise is seeded from a per-session RNG seed and is
//! deterministic (same canvas → same noise in same session; different session → different noise).
//!
//! This module is consumed by P3 when implementing `getImageData()` in JS bindings.
//! P1 provides the noise generator and pixel transformation.

use std::num::Wrapping;

/// Per-session canvas fingerprint noise generator.
///
/// Uses a simple LCG (linear congruential generator) seeded from a per-session u64.
/// Each call to `next_noise_u8()` advances the RNG state and returns a u8 noise value.
#[derive(Debug, Clone)]
pub struct CanvasNoiseGenerator {
    seed: u64,
    state: u64,
}

impl CanvasNoiseGenerator {
    /// Create a new noise generator with the given per-session seed.
    ///
    /// The seed is typically derived from BrowserSession's UUID or random number.
    /// Same seed → deterministic sequence of noise values.
    pub fn new(seed: u64) -> Self {
        Self {
            seed,
            state: seed,
        }
    }

    /// Generate the next noise value (0..=255).
    ///
    /// Uses an LCG: `state = (a * state + c) mod 2^64`.
    /// Parameters from Numerical Recipes (minimal, but sufficient for noise).
    fn next_u64(&mut self) -> u64 {
        const A: u64 = 6364136223846793005;
        const C: u64 = 1442695040888963407;
        self.state = (Wrapping(A) * Wrapping(self.state) + Wrapping(C)).0;
        self.state
    }

    /// Generate next noise byte (0..=255) clamped to safe range.
    ///
    /// Returns a u8 in range [0, 255] to be added (with wrapping) to pixel channels.
    pub fn next_noise_u8(&mut self) -> u8 {
        (self.next_u64() >> 8) as u8
    }

    /// Add per-channel noise to an RGBA pixel.
    ///
    /// Each of R, G, B channels is XORed with a noise byte (preserves alpha).
    /// Uses XOR instead of addition to avoid colour-space artifacts.
    pub fn apply_noise_to_pixel(&mut self, pixel: &mut [u8; 4]) {
        // Noise only R, G, B; preserve A
        pixel[0] ^= self.next_noise_u8();
        pixel[1] ^= self.next_noise_u8();
        pixel[2] ^= self.next_noise_u8();
    }

    /// Apply noise to an entire RGBA buffer (row-major, top-left origin).
    ///
    /// Iterates pixel-by-pixel and applies per-pixel XOR noise to R, G, B channels.
    pub fn apply_noise_to_buffer(&mut self, pixels: &mut [u8]) {
        for chunk in pixels.chunks_exact_mut(4) {
            if let Ok(pixel) = <&mut [u8; 4]>::try_from(chunk) {
                self.apply_noise_to_pixel(pixel);
            }
        }
    }

    /// Reset the RNG state to the seed (for reproducibility).
    ///
    /// Useful for applying the same noise sequence multiple times or resetting state.
    pub fn reset(&mut self) {
        self.state = self.seed;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_generator_deterministic() {
        let mut noise1 = CanvasNoiseGenerator::new(42);
        let mut noise2 = CanvasNoiseGenerator::new(42);

        for _ in 0..100 {
            assert_eq!(noise1.next_noise_u8(), noise2.next_noise_u8());
        }
    }

    #[test]
    fn noise_generator_different_seeds() {
        let mut noise1 = CanvasNoiseGenerator::new(42);
        let mut noise2 = CanvasNoiseGenerator::new(43);

        let mut found_diff = false;
        for _ in 0..100 {
            if noise1.next_noise_u8() != noise2.next_noise_u8() {
                found_diff = true;
                break;
            }
        }
        assert!(found_diff, "different seeds should produce different noise sequences");
    }

    #[test]
    fn noise_generator_reset() {
        let mut noise = CanvasNoiseGenerator::new(42);
        let first = (
            noise.next_noise_u8(),
            noise.next_noise_u8(),
            noise.next_noise_u8(),
        );

        noise.reset();
        let second = (
            noise.next_noise_u8(),
            noise.next_noise_u8(),
            noise.next_noise_u8(),
        );

        assert_eq!(first, second);
    }

    #[test]
    fn apply_noise_to_pixel_preserves_alpha() {
        let mut noise = CanvasNoiseGenerator::new(42);
        let mut pixel = [100u8, 150u8, 200u8, 255u8]; // R, G, B, A
        let alpha_before = pixel[3];
        noise.apply_noise_to_pixel(&mut pixel);
        assert_eq!(pixel[3], alpha_before, "alpha channel should be unchanged");
    }

    #[test]
    fn apply_noise_to_buffer_basic() {
        let mut noise = CanvasNoiseGenerator::new(42);
        let mut buffer = vec![100u8; 16]; // 4 pixels (4 bytes each), initially all 100
        for i in 0..4 {
            buffer[i * 4 + 3] = 255;
        }

        noise.apply_noise_to_buffer(&mut buffer);

        // Check that at least R, G, B changed (very unlikely all stay the same after XOR)
        let mut changed = false;
        for i in 0..4 {
            for ch in 0..3 {
                if buffer[i * 4 + ch] != 100 {
                    changed = true;
                    break;
                }
            }
        }
        assert!(changed, "at least some pixels should have changed");

        // Check alphas are still 255
        for i in 0..4 {
            assert_eq!(buffer[i * 4 + 3], 255, "pixel {} alpha should remain 255", i);
        }
    }

    #[test]
    fn apply_noise_xor_no_overflow() {
        let mut noise = CanvasNoiseGenerator::new(42);
        let mut pixel = [255u8, 255u8, 255u8, 255u8];
        noise.apply_noise_to_pixel(&mut pixel);
        // XOR with any u8 value produces a u8 — no overflow possible.
        // This test verifies the operation completes without panic.
        assert_eq!(pixel[3], 255, "alpha must remain unchanged");
    }
}
