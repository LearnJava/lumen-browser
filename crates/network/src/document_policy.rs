//! Document Policy header parser.
//! <https://wicg.github.io/document-policy/>
//!
//! Parses the `Document-Policy` (and `Document-Policy-Report-Only`) header —
//! a Structured Fields Dictionary (RFC 8941 §3.2) mapping feature names to
//! `sf-boolean` values (`?0`/`?1`). Unlike `Permissions-Policy`, a
//! Document-Policy feature has no per-origin allowlist, just an on/off
//! switch for the document that sent the header.
//!
//! GAP-POLICYREPORT (BUG-953): Lumen only *acts* on the `sync-xhr` feature
//! (`XMLHttpRequest.send()` with `async=false`), but the parser itself is
//! feature-agnostic — an unrecognised feature name simply has no reader.

use std::collections::HashMap;

/// Parsed representation of a `Document-Policy` (or `-Report-Only`) header.
///
/// Maps feature names (e.g. `"sync-xhr"`) to their boolean value. A feature
/// absent from the map is not restricted by this header — callers combine
/// this with their own per-feature default.
#[derive(Debug, Clone, Default)]
pub struct DocumentPolicy {
    /// Per-feature boolean values extracted from the header value.
    pub features: HashMap<String, bool>,
}

impl DocumentPolicy {
    /// `true` if `feature` was explicitly set to `?0` (disabled) by this
    /// policy. A feature absent from the header, or set to `?1`, is not
    /// disabled.
    pub fn feature_disabled(&self, feature: &str) -> bool {
        self.features.get(feature) == Some(&false)
    }
}

/// Parse the value of a `Document-Policy` (or `Document-Policy-Report-Only`)
/// header.
///
/// Syntax (RFC 8941 §3.2, boolean-valued members only): `feature=?0, …`.
/// Structured-field parameters after a `;` are ignored — Lumen's one
/// supported feature (`sync-xhr`) takes none. A value that isn't a bare
/// `?0`/`?1` boolean (e.g. a numeric threshold param some other feature
/// might use) is skipped rather than guessed at.
pub fn parse_document_policy_header(value: &str) -> DocumentPolicy {
    let mut policy = DocumentPolicy::default();
    for item in value.split(',') {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        let item = item.split(';').next().unwrap_or(item).trim();
        let Some((name, val)) = item.split_once('=') else {
            continue;
        };
        let feature = name.trim().to_ascii_lowercase();
        match val.trim() {
            "?0" => {
                policy.features.insert(feature, false);
            }
            "?1" => {
                policy.features.insert(feature, true);
            }
            _ => {}
        }
    }
    policy
}

// ── tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty_policy() {
        let p = parse_document_policy_header("");
        assert!(p.features.is_empty());
        assert!(!p.feature_disabled("sync-xhr"));
    }

    #[test]
    fn parse_disabled_feature() {
        let p = parse_document_policy_header("sync-xhr=?0");
        assert!(p.feature_disabled("sync-xhr"));
    }

    #[test]
    fn parse_enabled_feature_is_not_disabled() {
        let p = parse_document_policy_header("sync-xhr=?1");
        assert!(!p.feature_disabled("sync-xhr"));
    }

    #[test]
    fn absent_feature_is_not_disabled() {
        let p = parse_document_policy_header("other-feature=?0");
        assert!(!p.feature_disabled("sync-xhr"));
    }

    #[test]
    fn parse_multiple_features() {
        let p = parse_document_policy_header("sync-xhr=?0, other-feature=?1");
        assert!(p.feature_disabled("sync-xhr"));
        assert!(!p.feature_disabled("other-feature"));
    }

    #[test]
    fn feature_name_is_case_insensitive() {
        let p = parse_document_policy_header("Sync-XHR=?0");
        assert!(p.feature_disabled("sync-xhr"));
    }

    #[test]
    fn parameters_after_semicolon_are_ignored() {
        let p = parse_document_policy_header("sync-xhr=?0;report-to=default");
        assert!(p.feature_disabled("sync-xhr"));
    }

    #[test]
    fn non_boolean_value_is_skipped() {
        let p = parse_document_policy_header("sync-xhr=42");
        assert!(!p.feature_disabled("sync-xhr"));
        assert!(p.features.is_empty());
    }
}
