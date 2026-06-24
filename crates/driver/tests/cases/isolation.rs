//! Tests for per-context isolation (Task 8E, Phase 1).
//!
//! Validates that SessionContext properly isolates:
//! - Fingerprint profiles (Standard/Strict/Tor)
//! - User-Agent strings per profile and override
//! - Multiple sessions have independent contexts

use lumen_driver::{BrowserSession, FingerprintProfile, InProcessSession};

#[test]
fn test_default_fingerprint_profile() {
    let session = InProcessSession::new();
    assert_eq!(session.fingerprint_profile(), FingerprintProfile::Standard);
}

#[test]
fn test_fingerprint_profile_set() {
    let mut session = InProcessSession::new();
    assert!(session.set_fingerprint_profile(FingerprintProfile::Strict).is_ok());
    assert_eq!(session.fingerprint_profile(), FingerprintProfile::Strict);

    assert!(session.set_fingerprint_profile(FingerprintProfile::Tor).is_ok());
    assert_eq!(session.fingerprint_profile(), FingerprintProfile::Tor);

    assert!(session.set_fingerprint_profile(FingerprintProfile::Standard).is_ok());
    assert_eq!(session.fingerprint_profile(), FingerprintProfile::Standard);
}

#[test]
fn test_user_agent_default_standard() {
    let session = InProcessSession::new();
    let ua = session.user_agent();
    assert!(!ua.is_empty());
    assert!(ua.contains("Chrome") || ua.contains("Mozilla"));
}

#[test]
fn test_user_agent_default_strict() {
    let mut session = InProcessSession::new();
    session.set_fingerprint_profile(FingerprintProfile::Strict).unwrap();
    let ua = session.user_agent();
    assert!(!ua.is_empty());
    assert!(ua.contains("Firefox"));
}

#[test]
fn test_user_agent_default_tor() {
    let mut session = InProcessSession::new();
    session.set_fingerprint_profile(FingerprintProfile::Tor).unwrap();
    let ua = session.user_agent();
    assert!(!ua.is_empty());
    // Tor profile currently uses Firefox ESR base
    assert!(ua.contains("Firefox") || ua.contains("Mozilla"));
}

#[test]
fn test_user_agent_override() {
    let mut session = InProcessSession::new();

    let custom_ua = "CustomBot/1.0";
    assert!(session.set_user_agent(custom_ua).is_ok());
    assert_eq!(session.user_agent(), custom_ua);

    // Override should persist even when profile changes
    session.set_fingerprint_profile(FingerprintProfile::Strict).unwrap();
    assert_eq!(session.user_agent(), custom_ua);

    // Can be changed
    assert!(session.set_user_agent("AnotherBot/2.0").is_ok());
    assert_eq!(session.user_agent(), "AnotherBot/2.0");
}

#[test]
fn test_user_agent_empty_rejected() {
    let mut session = InProcessSession::new();
    let result = session.set_user_agent("");
    assert!(result.is_err());
    // User-Agent should not change on error
    assert!(!session.user_agent().is_empty());
}

#[test]
fn test_sessions_have_independent_contexts() {
    let mut session1 = InProcessSession::new();
    let session2 = InProcessSession::new();

    // Change session1's profile
    session1.set_fingerprint_profile(FingerprintProfile::Strict).unwrap();
    assert_eq!(session1.fingerprint_profile(), FingerprintProfile::Strict);

    // session2 should still be Standard
    assert_eq!(session2.fingerprint_profile(), FingerprintProfile::Standard);

    // Change session1's user-agent
    session1.set_user_agent("Session1UA").unwrap();
    assert_eq!(session1.user_agent(), "Session1UA");

    // session2 should have different user-agent (default for Standard)
    assert_ne!(session2.user_agent(), "Session1UA");
    assert!(session2.user_agent().contains("Chrome"));
}

#[test]
fn test_profile_switch_changes_default_ua() {
    let mut session = InProcessSession::new();

    let standard_ua = session.user_agent();
    assert!(standard_ua.contains("Chrome"));

    session.set_fingerprint_profile(FingerprintProfile::Strict).unwrap();
    let strict_ua = session.user_agent();
    assert!(strict_ua.contains("Firefox"));

    // UAs should be different
    assert_ne!(standard_ua, strict_ua);

    // Switch back
    session.set_fingerprint_profile(FingerprintProfile::Standard).unwrap();
    assert_eq!(session.user_agent(), standard_ua);
}
