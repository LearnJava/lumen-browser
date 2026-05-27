//! Isolated context for a single BrowserSession.
//!
//! `SessionContext` encapsulates all per-session state that must not be shared
//! between BrowserSession instances: cookies, storage, HTTP cache, fingerprint profile.
//!
//! Architecture: [`SessionContext`] is held as a private field in [`InProcessSession`](crate::InProcessSession)
//! and similar implementations. This separates DOM/layout state from resource isolation.

use lumen_core::error::Result;

use crate::FingerprintProfile;

/// Isolated context for a single BrowserSession.
///
/// Contains cookies, storage backends, HTTP cache, and fingerprint profile that must not be
/// shared between sessions. Each BrowserSession instance has its own SessionContext.
///
/// # Phase 1 (8E, May 2026)
///
/// Currently implements:
/// - Per-session fingerprint profile (Standard/Strict/Tor).
/// - Per-session User-Agent override.
/// - Placeholder for future: cookies, storage, HTTP cache (wired in Phase 1 implementation).
///
/// # Future (Phase 2+)
///
/// - Per-session CookieJar (origin-keyed).
/// - Per-session StorageBackend (localStorage, sessionStorage per origin).
/// - Per-session HttpCache.
/// - Image decode cache (task 10E).
/// - Glyph atlas eviction (task 10G).
pub struct SessionContext {
    /// Fingerprint profile: Standard/Strict/Tor. Default: Standard.
    fingerprint_profile: FingerprintProfile,

    /// User-Agent override. If None, uses default for current profile.
    user_agent_override: Option<String>,
}

impl SessionContext {
    /// Create a new SessionContext with default settings (Standard profile).
    pub fn new() -> Self {
        Self {
            fingerprint_profile: FingerprintProfile::Standard,
            user_agent_override: None,
        }
    }

    /// Create a SessionContext with a specific fingerprint profile.
    pub fn with_fingerprint_profile(profile: FingerprintProfile) -> Self {
        Self {
            fingerprint_profile: profile,
            user_agent_override: None,
        }
    }

    /// Get the current fingerprint profile.
    pub fn fingerprint_profile(&self) -> FingerprintProfile {
        self.fingerprint_profile
    }

    /// Set the fingerprint profile.
    pub fn set_fingerprint_profile(&mut self, profile: FingerprintProfile) {
        self.fingerprint_profile = profile;
    }

    /// Get the User-Agent string: either the override or the default for current profile.
    pub fn user_agent(&self) -> String {
        self.user_agent_override
            .clone()
            .unwrap_or_else(|| default_user_agent(self.fingerprint_profile))
    }

    /// Set a User-Agent override. If set, this takes precedence over profile defaults.
    pub fn set_user_agent(&mut self, ua: &str) -> Result<()> {
        if ua.is_empty() {
            return Err(lumen_core::error::Error::Other(
                "User-Agent must not be empty".to_string(),
            ));
        }
        self.user_agent_override = Some(ua.to_string());
        Ok(())
    }

    /// Clear the User-Agent override, reverting to profile default.
    pub fn clear_user_agent_override(&mut self) {
        self.user_agent_override = None;
    }
}

impl Default for SessionContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Get the default User-Agent string for a given fingerprint profile.
///
/// - **Standard**: Chrome 126 user-agent (current as of May 2026).
/// - **Strict**: Firefox ESR user-agent.
/// - **Tor**: Tor Browser user-agent (Phase 3+).
fn default_user_agent(profile: FingerprintProfile) -> String {
    match profile {
        FingerprintProfile::Standard => {
            // Chrome 126 (May 2026). Source: Chrome release notes.
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36".to_string()
        }
        FingerprintProfile::Strict => {
            // Firefox ESR (May 2026).
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:128.0) Gecko/20100101 Firefox/128.0".to_string()
        }
        FingerprintProfile::Tor => {
            // Tor Browser (Phase 3+): same as Firefox ESR base.
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:128.0) Gecko/20100101 Firefox/128.0".to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_context_default() {
        let ctx = SessionContext::new();
        assert_eq!(ctx.fingerprint_profile(), FingerprintProfile::Standard);
        assert!(!ctx.user_agent().is_empty());
    }

    #[test]
    fn test_session_context_with_profile() {
        let ctx = SessionContext::with_fingerprint_profile(FingerprintProfile::Strict);
        assert_eq!(ctx.fingerprint_profile(), FingerprintProfile::Strict);
    }

    #[test]
    fn test_user_agent_override() {
        let mut ctx = SessionContext::new();
        let original = ctx.user_agent();

        ctx.set_user_agent("CustomUA/1.0").unwrap();
        assert_eq!(ctx.user_agent(), "CustomUA/1.0");

        ctx.clear_user_agent_override();
        assert_eq!(ctx.user_agent(), original);
    }

    #[test]
    fn test_user_agent_empty_invalid() {
        let mut ctx = SessionContext::new();
        let result = ctx.set_user_agent("");
        assert!(result.is_err());
    }

    #[test]
    fn test_fingerprint_profile_change() {
        let mut ctx = SessionContext::new();
        let standard_ua = ctx.user_agent();

        ctx.set_fingerprint_profile(FingerprintProfile::Strict);
        let strict_ua = ctx.user_agent();

        assert_ne!(standard_ua, strict_ua);
        assert_eq!(ctx.fingerprint_profile(), FingerprintProfile::Strict);
    }

    #[test]
    fn test_user_agent_override_survives_profile_change() {
        let mut ctx = SessionContext::new();
        ctx.set_user_agent("CustomUA/2.0").unwrap();

        ctx.set_fingerprint_profile(FingerprintProfile::Strict);
        assert_eq!(ctx.user_agent(), "CustomUA/2.0");
    }
}
