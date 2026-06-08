//! Deterministic mode (8F) for BrowserSession — clock control, RNG seeding,
//! and fingerprint freezing.
//!
//! # Overview
//!
//! Deterministic mode makes browser behaviour reproducible across test runs:
//!
//! | Feature | Default (off) | Deterministic (on) |
//! |---|---|---|
//! | `Date.now()` / `performance.now()` | System wall-clock | Frozen at `ms` or monotonic |
//! | `Math.random()` | OS entropy | Seeded xorshift32 PRNG |
//! | Canvas fingerprint | Platform-dependent | Fixed blank hash |
//! | WebGL vendor/renderer | GPU-reported strings | Normalized via `GpuFingerprint` |
//! | Audio fingerprint | Platform-dependent | Zero-valued samples |
//! | Font enumeration | System font list | Fixed single-font list (Inter) |
//!
//! # Usage
//!
//! ```rust,no_run
//! use lumen_driver::{BrowserSession, InProcessSession};
//! use lumen_driver::determinism::DeterministicConfig;
//!
//! let mut session = InProcessSession::new();
//! let cfg = DeterministicConfig::default();
//! cfg.apply(&mut session).unwrap();
//! session.navigate("file:///path/to/page.html").unwrap();
//! ```

pub use lumen_core::ext::ClockMode;

use lumen_core::error::Result;

use crate::{BrowserSession, FingerprintProfile};

/// Configuration bundle for enabling deterministic mode on a `BrowserSession`.
///
/// Apply with [`DeterministicConfig::apply`] before calling `navigate()`.
#[derive(Debug, Clone)]
pub struct DeterministicConfig {
    /// Clock mode: `Frozen(0)` by default (Date.now() returns 0).
    pub clock: ClockMode,
    /// RNG seed: `Some(1)` by default (reproducible Math.random() sequence).
    pub rng_seed: Option<u64>,
    /// Fingerprint profile to freeze: `None` = leave current profile, do not freeze.
    ///
    /// When `Some(profile)`, sets the profile and prevents further changes so that
    /// canvas/WebGL/audio/font fingerprinting APIs return fixed, profile-appropriate values.
    pub freeze_fingerprint: Option<FingerprintProfile>,
}

impl Default for DeterministicConfig {
    fn default() -> Self {
        Self {
            clock: ClockMode::Frozen(0),
            rng_seed: Some(1),
            freeze_fingerprint: None,
        }
    }
}

impl DeterministicConfig {
    /// Convenience constructor: fully deterministic mode with a specific RNG seed.
    ///
    /// Freezes clock at 0 and sets the given seed. Fingerprint is not changed.
    pub fn with_seed(seed: u64) -> Self {
        Self {
            clock: ClockMode::Frozen(0),
            rng_seed: Some(seed),
            freeze_fingerprint: None,
        }
    }

    /// Convenience constructor for snapshot testing.
    ///
    /// Freezes clock at 0, seeds RNG at 42, and freezes fingerprint at `Standard`
    /// to pin canvas/WebGL/audio/font APIs to deterministic values.
    pub fn for_snapshot() -> Self {
        Self {
            clock: ClockMode::Frozen(0),
            rng_seed: Some(42),
            freeze_fingerprint: Some(FingerprintProfile::Standard),
        }
    }

    /// Apply this configuration to `session`.
    ///
    /// Order: clock → rng_seed → freeze_fingerprint.
    /// Returns the first error if any step fails.
    pub fn apply(&self, session: &mut dyn BrowserSession) -> Result<()> {
        session.set_clock(self.clock)?;
        session.set_rng_seed(self.rng_seed)?;
        if let Some(profile) = self.freeze_fingerprint {
            session.freeze_fingerprint(profile)?;
        }
        Ok(())
    }
}

/// Returns a deterministic u64 seed derived from a URL string.
///
/// Uses FNV-1a hash so the same URL always produces the same seed across platforms.
/// Used by the shell `--deterministic` mode to seed Math.random() per-page.
pub fn seed_from_url(url: &str) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = FNV_OFFSET;
    for byte in url.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_from_url_is_deterministic() {
        let s1 = seed_from_url("https://example.com/page");
        let s2 = seed_from_url("https://example.com/page");
        assert_eq!(s1, s2);
    }

    #[test]
    fn seed_from_url_differs_for_different_urls() {
        let s1 = seed_from_url("https://example.com/a");
        let s2 = seed_from_url("https://example.com/b");
        assert_ne!(s1, s2);
    }

    #[test]
    fn default_config_has_frozen_clock_and_seed() {
        let cfg = DeterministicConfig::default();
        assert_eq!(cfg.clock, ClockMode::Frozen(0));
        assert_eq!(cfg.rng_seed, Some(1));
        assert!(cfg.freeze_fingerprint.is_none());
    }

    #[test]
    fn with_seed_config() {
        let cfg = DeterministicConfig::with_seed(9999);
        assert_eq!(cfg.rng_seed, Some(9999));
        assert_eq!(cfg.clock, ClockMode::Frozen(0));
    }

    #[test]
    fn for_snapshot_config_freezes_fingerprint() {
        let cfg = DeterministicConfig::for_snapshot();
        assert_eq!(cfg.freeze_fingerprint, Some(FingerprintProfile::Standard));
        assert_eq!(cfg.rng_seed, Some(42));
    }
}
