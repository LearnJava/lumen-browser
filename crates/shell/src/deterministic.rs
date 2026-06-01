//! Deterministic render mode (8F).
//!
//! When `--deterministic` is present:
//! - `Date.now()` is frozen at 0 (epoch)
//! - `Math.random()` is replaced with a seeded xorshift32 PRNG (seed from URL hash)
//! - `requestAnimationFrame` callbacks receive a fixed 0 ms timestamp (no wall-clock jitter)
//! - The browser window opens at 1280×800 instead of the default 1024×720

/// Extracts the `--deterministic` flag from a CLI arg list.
///
/// Returns `(found, remaining_args)` — the flag is removed from the returned vec.
pub fn extract_deterministic(args: &[String]) -> (bool, Vec<String>) {
    let found = args.iter().any(|a| a == "--deterministic");
    let rest = args.iter().filter(|a| *a != "--deterministic").cloned().collect();
    (found, rest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_removes_flag() {
        let args: Vec<String> = vec!["--deterministic".into(), "http://x.com".into()];
        let (found, rest) = extract_deterministic(&args);
        assert!(found);
        assert_eq!(rest, vec!["http://x.com".to_string()]);
    }

    #[test]
    fn extract_not_present() {
        let args: Vec<String> = vec!["http://x.com".into()];
        let (found, rest) = extract_deterministic(&args);
        assert!(!found);
        assert_eq!(rest, vec!["http://x.com".to_string()]);
    }

    #[test]
    fn extract_empty_args() {
        let (found, rest) = extract_deterministic(&[]);
        assert!(!found);
        assert!(rest.is_empty());
    }
}
