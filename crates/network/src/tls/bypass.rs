//! Session-scoped "proceed anyway" cert-error bypass registry (ph3-tls-hardening A6).
//!
//! When the shell's cert interstitial (`crates/shell/src/panels/cert_interstitial.rs`)
//! offers the user a "Proceed anyway" action, the chosen origin's host is
//! recorded here. [`LumenVerifier`](super::verifier::LumenVerifier) consults
//! this set on every subsequent handshake for that host and turns a chain
//! validation / stapled-OCSP failure back into a pass — exactly the informed,
//! user-initiated risk acceptance a real cert-error interstitial grants.
//!
//! Process-global and in-memory only (no persistence across restarts), which
//! matches the "session override" scope the task calls for: `tls_config_for_profile`
//! already caches one `ClientConfig`/`LumenVerifier` per [`crate::tls::TlsProfile`]
//! for the whole process lifetime, so a per-connection or per-profile store
//! could never be read back by a later, unrelated connection on the same
//! profile — a process-wide set keyed by hostname is the only shape that
//! actually reaches every future handshake to that host.

use std::collections::HashSet;
use std::sync::{OnceLock, RwLock};

fn registry() -> &'static RwLock<HashSet<String>> {
    static REGISTRY: OnceLock<RwLock<HashSet<String>>> = OnceLock::new();
    REGISTRY.get_or_init(|| RwLock::new(HashSet::new()))
}

/// Record that the user chose "Proceed anyway" for `host` — every later
/// handshake to this exact hostname (case-sensitive; callers should pass an
/// already-lowercased host, as DNS names conventionally are) skips cert
/// verification failures instead of hard-failing.
///
/// BUG-1615: a Known HSTS Host must never be bypassable (RFC 6797 §8.4/§12.1),
/// so a host on the built-in preload list is refused here and `false` is
/// returned. Learned (dynamic) HSTS entries live in the shell's store, which
/// the caller checks before getting here
/// ([`HttpClient::is_known_hsts_host`](crate::HttpClient::is_known_hsts_host)).
pub fn allow_host(host: &str) -> bool {
    if is_hsts_preloaded(host) {
        return false;
    }
    if let Ok(mut set) = registry().write() {
        set.insert(host.to_owned());
        return true;
    }
    false
}

/// Whether `host` has an active "Proceed anyway" override for this session.
/// Always `false` for a preloaded HSTS host, even if it was recorded some
/// other way.
pub fn is_allowed(host: &str) -> bool {
    !is_hsts_preloaded(host) && registry().read().is_ok_and(|set| set.contains(host))
}

fn is_hsts_preloaded(host: &str) -> bool {
    crate::get_preload_list().is_preloaded(host)
}

/// Clear every recorded override. Test-only — production has no UI to revoke
/// a single host's bypass mid-session (closing the tab/process is the reset).
#[cfg(test)]
pub fn clear_for_test() {
    if let Ok(mut set) = registry().write() {
        set.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // The registry is process-global `static` state — tests running in
    // parallel threads (the default `cargo test` runner) would otherwise
    // race on `allow_host`/`clear_for_test`. Serialize this module's tests.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn unknown_host_not_allowed() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_for_test();
        assert!(!is_allowed("never-allowed.example"));
    }

    #[test]
    fn allowed_host_is_reported_allowed() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_for_test();
        allow_host("bad-cert.example");
        assert!(is_allowed("bad-cert.example"));
        assert!(!is_allowed("other.example"));
    }

    #[test]
    fn clear_for_test_resets_registry() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        allow_host("temp.example");
        clear_for_test();
        assert!(!is_allowed("temp.example"));
    }

    #[test]
    fn preloaded_hsts_host_is_never_bypassed() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_for_test();
        // Any host from the embedded preload list; google.com is in it.
        assert!(crate::get_preload_list().is_preloaded("google.com"));
        assert!(!allow_host("google.com"));
        assert!(!is_allowed("google.com"));
    }
}
