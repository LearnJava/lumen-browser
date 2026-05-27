//! Isolated context for a single BrowserSession.
//!
//! SessionContext encapsulates per-session state:
//! cookies, storage, HTTP cache, fingerprint profile.
//!
//! Architecture: SessionContext is held as a private field in InProcessSession
//! and similar implementations. This separates DOM/layout state from resource isolation.

use std::collections::HashMap;
use lumen_core::error::Result;

use crate::FingerprintProfile;

type CookieStore = HashMap<(String, String), String>;
type HttpCache = HashMap<String, Vec<u8>>;

/// Isolated context for a single BrowserSession.
/// # Phase 1b (8E, May 2026)
/// Implements: fingerprint profile, User-Agent, cookies, HTTP cache, storage
/// # Phase 1c (8F, May 2026)
/// Extends: deterministic mode (frozen clock, RNG seed, fingerprint lock)
pub struct SessionContext {
    fingerprint_profile: FingerprintProfile,
    user_agent_override: Option<String>,
    cookies: CookieStore,
    http_cache: HttpCache,
    storage: HashMap<String, HashMap<String, Vec<u8>>>,
    /// Frozen clock timestamp (ms since epoch). None = use system clock.
    frozen_clock_ms: Option<u64>,
    /// RNG seed for deterministic randomness. None = use OS entropy.
    rng_seed: Option<u64>,
    /// If true, fingerprint profile changes are rejected (freeze current profile).
    fingerprint_frozen: bool,
}

impl SessionContext {
    pub fn new() -> Self {
        Self {
            fingerprint_profile: FingerprintProfile::Standard,
            user_agent_override: None,
            cookies: CookieStore::new(),
            http_cache: HttpCache::new(),
            storage: HashMap::new(),
            frozen_clock_ms: None,
            rng_seed: None,
            fingerprint_frozen: false,
        }
    }

    pub fn with_fingerprint_profile(profile: FingerprintProfile) -> Self {
        Self {
            fingerprint_profile: profile,
            user_agent_override: None,
            cookies: CookieStore::new(),
            http_cache: HttpCache::new(),
            storage: HashMap::new(),
            frozen_clock_ms: None,
            rng_seed: None,
            fingerprint_frozen: false,
        }
    }

    pub fn fingerprint_profile(&self) -> FingerprintProfile {
        self.fingerprint_profile
    }

    pub fn set_fingerprint_profile(&mut self, profile: FingerprintProfile) -> Result<()> {
        if self.fingerprint_frozen {
            return Err(lumen_core::error::Error::Other(
                "Fingerprint profile is frozen".to_string(),
            ));
        }
        self.fingerprint_profile = profile;
        Ok(())
    }

    pub fn user_agent(&self) -> String {
        self.user_agent_override
            .clone()
            .unwrap_or_else(|| default_user_agent(self.fingerprint_profile))
    }

    pub fn set_user_agent(&mut self, ua: &str) -> Result<()> {
        if ua.is_empty() {
            return Err(lumen_core::error::Error::Other(
                "User-Agent must not be empty".to_string(),
            ));
        }
        self.user_agent_override = Some(ua.to_string());
        Ok(())
    }

    pub fn clear_user_agent_override(&mut self) {
        self.user_agent_override = None;
    }

    /// Get current frozen clock timestamp (ms since epoch), or None if system clock is used.
    pub fn frozen_clock_ms(&self) -> Option<u64> {
        self.frozen_clock_ms
    }

    /// Set frozen clock to a specific timestamp (ms since epoch) for deterministic testing.
    /// Once set, all `Date.now()` / `performance.now()` calls use this value (not advancing).
    pub fn set_frozen_clock(&mut self, timestamp_ms: u64) {
        self.frozen_clock_ms = Some(timestamp_ms);
    }

    /// Clear frozen clock; resume using system time.
    pub fn clear_frozen_clock(&mut self) {
        self.frozen_clock_ms = None;
    }

    /// Get RNG seed for deterministic randomness, or None if OS entropy is used.
    pub fn rng_seed(&self) -> Option<u64> {
        self.rng_seed
    }

    /// Set RNG seed for deterministic random numbers in JS Math.random() and crypto.getRandomValues().
    /// Used for repeatable test automation.
    pub fn set_rng_seed(&mut self, seed: u64) {
        self.rng_seed = Some(seed);
    }

    /// Clear RNG seed; resume using OS entropy.
    pub fn clear_rng_seed(&mut self) {
        self.rng_seed = None;
    }

    /// Check if fingerprint profile is frozen (cannot be changed).
    pub fn is_fingerprint_frozen(&self) -> bool {
        self.fingerprint_frozen
    }

    /// Freeze current fingerprint profile: prevent further changes to set_fingerprint_profile().
    /// Used to ensure consistent fingerprint across multiple test iterations.
    pub fn freeze_fingerprint(&mut self) {
        self.fingerprint_frozen = true;
    }

    /// Unfreeze fingerprint profile; allow changes again.
    pub fn unfreeze_fingerprint(&mut self) {
        self.fingerprint_frozen = false;
    }

    pub fn get_cookies_for_request(&self, origin: &str, path: &str) -> String {
        let prefix = (origin.to_string(), path.to_string());
        self.cookies.get(&prefix).cloned().unwrap_or_default()
    }

    pub fn process_set_cookie(&mut self, origin: &str, path: &str, cookie_header: &str) {
        let key = (origin.to_string(), path.to_string());
        let existing = self.cookies.get(&key).cloned().unwrap_or_default();
        let separator = if existing.is_empty() { "" } else { "; " };
        self.cookies.insert(key, format!("{}{}{}", existing, separator, cookie_header));
    }

    pub fn clear_cookies(&mut self) {
        self.cookies.clear();
    }

    pub fn get_storage(&self, origin: &str, key: &str) -> Option<Vec<u8>> {
        self.storage
            .get(origin)
            .and_then(|store| store.get(key).cloned())
    }

    pub fn set_storage(&mut self, origin: &str, key: String, value: Vec<u8>) {
        self.storage
            .entry(origin.to_string())
            .or_insert_with(HashMap::new)
            .insert(key, value);
    }

    pub fn clear_origin_storage(&mut self, origin: &str) {
        self.storage.remove(origin);
    }

    pub fn clear_all_storage(&mut self) {
        self.storage.clear();
    }

    pub fn storage_keys(&self, origin: &str) -> Vec<String> {
        self.storage
            .get(origin)
            .map(|store| store.keys().cloned().collect())
            .unwrap_or_default()
    }

    pub fn get_cached_response(&self, url: &str) -> Option<Vec<u8>> {
        self.http_cache.get(url).cloned()
    }

    pub fn cache_response(&mut self, url: String, body: Vec<u8>) {
        self.http_cache.insert(url, body);
    }

    pub fn clear_http_cache(&mut self) {
        self.http_cache.clear();
    }
}

impl Default for SessionContext {
    fn default() -> Self {
        Self::new()
    }
}

fn default_user_agent(profile: FingerprintProfile) -> String {
    match profile {
        FingerprintProfile::Standard => {
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36".to_string()
        }
        FingerprintProfile::Strict => {
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:128.0) Gecko/20100101 Firefox/128.0".to_string()
        }
        FingerprintProfile::Tor => {
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:128.0) Gecko/20100101 Firefox/128.0".to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cookies_isolated() {
        let mut ctx1 = SessionContext::new();
        let ctx2 = SessionContext::new();
        ctx1.process_set_cookie("https://example.com", "/", "session=abc123");
        assert_eq!(ctx1.get_cookies_for_request("https://example.com", "/"), "session=abc123");
        assert_eq!(ctx2.get_cookies_for_request("https://example.com", "/"), "");
    }

    #[test]
    fn test_storage_isolated() {
        let mut ctx1 = SessionContext::new();
        let ctx2 = SessionContext::new();
        ctx1.set_storage("https://example.com", "key1".to_string(), b"value1".to_vec());
        assert_eq!(ctx1.get_storage("https://example.com", "key1"), Some(b"value1".to_vec()));
        assert_eq!(ctx2.get_storage("https://example.com", "key1"), None);
    }

    #[test]
    fn test_http_cache_isolated() {
        let mut ctx1 = SessionContext::new();
        let ctx2 = SessionContext::new();
        ctx1.cache_response("https://example.com/page".to_string(), b"content1".to_vec());
        assert_eq!(ctx1.get_cached_response("https://example.com/page"), Some(b"content1".to_vec()));
        assert_eq!(ctx2.get_cached_response("https://example.com/page"), None);
    }
}
