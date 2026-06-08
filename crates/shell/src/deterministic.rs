//! Deterministic render mode (8F) — CLI flag parsing.
//!
//! When `--deterministic` is present:
//! - `Date.now()` is frozen at 0 (epoch)
//! - `Math.random()` is replaced with a seeded xorshift32 PRNG (seed from URL hash)
//! - `requestAnimationFrame` callbacks receive a fixed 0 ms timestamp (no wall-clock jitter)
//! - The browser window opens at 1280×800 instead of the default 1024×720
//!
//! Additional flags (can be combined with or without `--deterministic`):
//! - `--rng-seed <N>` — override the RNG seed with a specific u64 value
//! - `--monotonic-clock` — use a monotonically increasing clock (1 ms per tick) instead of frozen

/// Parsed deterministic-mode configuration from CLI args.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct DetConfig {
    /// `--deterministic`: master switch — freeze clock at 0, seed RNG from URL hash.
    pub enabled: bool,
    /// `--rng-seed <N>`: explicit RNG seed (overrides URL-hash derivation).
    pub rng_seed: Option<u64>,
    /// `--monotonic-clock`: each Date.now()/performance.now() call advances by 1 ms.
    pub monotonic_clock: bool,
}

/// Extract all deterministic-mode flags from CLI args.
///
/// Removes recognised flags from the returned vec; unknown args pass through unchanged.
pub fn extract_deterministic(args: &[String]) -> (DetConfig, Vec<String>) {
    let mut cfg = DetConfig::default();
    let mut rest: Vec<String> = Vec::new();
    let mut iter = args.iter();

    while let Some(arg) = iter.next() {
        if arg == "--deterministic" {
            cfg.enabled = true;
        } else if arg == "--monotonic-clock" {
            cfg.monotonic_clock = true;
            cfg.enabled = true; // implies deterministic
        } else if arg == "--rng-seed" {
            if let Some(val) = iter.next() {
                if let Ok(n) = val.parse::<u64>() {
                    cfg.rng_seed = Some(n);
                } else {
                    // Unrecognised value — keep both args.
                    rest.push(arg.clone());
                    rest.push(val.clone());
                }
            } else {
                rest.push(arg.clone());
            }
        } else {
            rest.push(arg.clone());
        }
    }

    (cfg, rest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_removes_flag() {
        let args: Vec<String> = vec!["--deterministic".into(), "http://x.com".into()];
        let (cfg, rest) = extract_deterministic(&args);
        assert!(cfg.enabled);
        assert_eq!(rest, vec!["http://x.com".to_string()]);
    }

    #[test]
    fn extract_not_present() {
        let args: Vec<String> = vec!["http://x.com".into()];
        let (cfg, rest) = extract_deterministic(&args);
        assert!(!cfg.enabled);
        assert_eq!(rest, vec!["http://x.com".to_string()]);
    }

    #[test]
    fn extract_empty_args() {
        let (cfg, rest) = extract_deterministic(&[]);
        assert!(!cfg.enabled);
        assert!(rest.is_empty());
    }

    #[test]
    fn extract_rng_seed() {
        let args: Vec<String> = vec!["--rng-seed".into(), "12345".into(), "http://x.com".into()];
        let (cfg, rest) = extract_deterministic(&args);
        assert_eq!(cfg.rng_seed, Some(12345));
        assert_eq!(rest, vec!["http://x.com".to_string()]);
    }

    #[test]
    fn extract_monotonic_clock_implies_deterministic() {
        let args: Vec<String> = vec!["--monotonic-clock".into()];
        let (cfg, rest) = extract_deterministic(&args);
        assert!(cfg.enabled);
        assert!(cfg.monotonic_clock);
        assert!(rest.is_empty());
    }

}
