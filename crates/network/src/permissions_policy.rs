//! Permissions Policy (formerly Feature Policy) parser.
//! <https://www.w3.org/TR/permissions-policy/>
//!
//! Parses the `Permissions-Policy` header and the legacy `Feature-Policy`
//! header into a structured [`PermissionsPolicy`].
//!
//! Phase 0: parsing + data model only.  Enforcement (blocking feature access
//! per origin) is wired by the shell in Phase 1.

use std::collections::HashMap;

/// The allowlist for a single feature in a [`PermissionsPolicy`].
#[derive(Debug, Clone, PartialEq)]
pub enum PermissionsAllowlist {
    /// `*` — all origins are allowed to use the feature.
    All,
    /// `()` — the feature is disabled for all origins.
    None,
    /// An explicit list of origins (may include the token `"self"`).
    Origins(Vec<String>),
}

/// Parsed representation of a `Permissions-Policy` (or `Feature-Policy`) header.
///
/// Maps feature names (e.g. `"camera"`, `"geolocation"`) to their allowlists.
/// Features not present in the map default to `*` (all origins allowed) per the spec.
#[derive(Debug, Clone, Default)]
pub struct PermissionsPolicy {
    /// Per-feature allowlists extracted from the header value.
    pub features: HashMap<String, PermissionsAllowlist>,
}

impl PermissionsPolicy {
    /// Returns `true` if `feature` is allowed for the given `origin`.
    ///
    /// Phase 0: if the origin matches `"self"` or the allowlist includes it, returns
    /// `true`; `None` (`()`) returns `false`; everything else returns `true`.
    pub fn allows_feature(&self, feature: &str, origin: Option<&str>) -> bool {
        match self.features.get(feature) {
            None => true,
            Some(PermissionsAllowlist::All) => true,
            Some(PermissionsAllowlist::None) => false,
            Some(PermissionsAllowlist::Origins(origins)) => {
                let target = origin.unwrap_or("self");
                origins.iter().any(|o| o == "*" || o == target)
            }
        }
    }

    /// Returns all feature names listed in this policy.
    pub fn features(&self) -> Vec<&str> {
        self.features.keys().map(String::as_str).collect()
    }

    /// Returns feature names for which the current document origin (`"self"`) is allowed.
    pub fn allowed_features(&self) -> Vec<&str> {
        self.features
            .iter()
            .filter(|(_, al)| match al {
                PermissionsAllowlist::None => false,
                PermissionsAllowlist::All => true,
                PermissionsAllowlist::Origins(v) => v.iter().any(|o| o == "self" || o == "*"),
            })
            .map(|(k, _)| k.as_str())
            .collect()
    }
}

/// Parse the value of a `Permissions-Policy` header.
///
/// Syntax (Structured Fields §3): `feature=(token …), …`
/// Examples:
/// - `camera=*` — allow all origins
/// - `microphone=()` — disable for all origins
/// - `geolocation=(self "https://example.com")` — allow self + specific origin
pub fn parse_permissions_policy_header(value: &str) -> PermissionsPolicy {
    let mut policy = PermissionsPolicy::default();
    for item in value.split(',') {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        if let Some((name, rest)) = item.split_once('=') {
            let feature = name.trim().to_ascii_lowercase();
            let allowlist = parse_allowlist(rest.trim());
            policy.features.insert(feature, allowlist);
        }
    }
    policy
}

/// Parse the legacy `Feature-Policy` header (space-separated, semicolon-delimited).
///
/// Syntax: `feature 'value'; …`
/// Example: `camera *; microphone 'self'; geolocation 'none'`
pub fn parse_feature_policy_header(value: &str) -> PermissionsPolicy {
    let mut policy = PermissionsPolicy::default();
    for item in value.split(';') {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        let mut parts = item.splitn(2, char::is_whitespace);
        let feature = match parts.next() {
            Some(f) => f.trim().to_ascii_lowercase(),
            None => continue,
        };
        let rest = parts.next().unwrap_or("*").trim();
        let allowlist = parse_legacy_allowlist(rest);
        policy.features.insert(feature, allowlist);
    }
    policy
}

// ── internal helpers ─────────────────────────────────────────────────────────

/// Parse a Permissions-Policy structured-field allowlist token.
///
/// `*` → All, `()` → None, `(self "https://…")` → Origins.
fn parse_allowlist(s: &str) -> PermissionsAllowlist {
    if s == "*" {
        return PermissionsAllowlist::All;
    }
    // Strip surrounding parens if present.
    let inner = if s.starts_with('(') && s.ends_with(')') {
        &s[1..s.len() - 1]
    } else {
        s
    };
    let inner = inner.trim();
    if inner.is_empty() {
        return PermissionsAllowlist::None;
    }
    // Split on whitespace; strip surrounding quotes from origins.
    let origins: Vec<String> = inner
        .split_whitespace()
        .map(|tok| tok.trim_matches('"').to_owned())
        .collect();
    PermissionsAllowlist::Origins(origins)
}

/// Parse a legacy Feature-Policy allowlist token.
///
/// `*` → All, `'none'` → None, `'self'` → Origins(["self"]).
fn parse_legacy_allowlist(s: &str) -> PermissionsAllowlist {
    let tok = s.trim_matches('\'');
    match tok {
        "*" => PermissionsAllowlist::All,
        "none" => PermissionsAllowlist::None,
        "self" => PermissionsAllowlist::Origins(vec!["self".to_owned()]),
        other => PermissionsAllowlist::Origins(vec![other.to_owned()]),
    }
}

// ── tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty_policy() {
        let p = parse_permissions_policy_header("");
        assert!(p.features.is_empty());
        // Unknown features default to allowed.
        assert!(p.allows_feature("camera", None));
    }

    #[test]
    fn parse_disabled_feature() {
        let p = parse_permissions_policy_header("geolocation=()");
        assert_eq!(p.features["geolocation"], PermissionsAllowlist::None);
        assert!(!p.allows_feature("geolocation", None));
    }

    #[test]
    fn parse_self_origin() {
        let p = parse_permissions_policy_header("microphone=(self)");
        assert!(p.allows_feature("microphone", Some("self")));
        assert!(!p.allows_feature("microphone", Some("https://example.com")));
    }

    #[test]
    fn parse_multiple_features() {
        let p = parse_permissions_policy_header(
            "camera=*, microphone=(), geolocation=(self \"https://example.com\")",
        );
        assert_eq!(p.features["camera"], PermissionsAllowlist::All);
        assert_eq!(p.features["microphone"], PermissionsAllowlist::None);
        assert!(p.allows_feature("camera", Some("https://other.com")));
        assert!(!p.allows_feature("microphone", Some("self")));
        assert!(p.allows_feature("geolocation", Some("https://example.com")));
    }

    #[test]
    fn allowed_features_returns_non_none() {
        let p = parse_permissions_policy_header("camera=*, microphone=(), usb=(self)");
        let allowed = p.allowed_features();
        assert!(allowed.contains(&"camera"));
        assert!(!allowed.contains(&"microphone"));
        assert!(allowed.contains(&"usb"));
    }

    #[test]
    fn parse_legacy_feature_policy() {
        let p = parse_feature_policy_header("camera *; microphone 'self'; geolocation 'none'");
        assert_eq!(p.features["camera"], PermissionsAllowlist::All);
        assert!(!p.allows_feature("geolocation", None));
        assert!(p.allows_feature("microphone", Some("self")));
    }
}
