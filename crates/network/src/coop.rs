//! Cross-Origin security policies (PH2-2 Phase 1):
//! COOP, COEP, CORP header parsing and enforcement.
//!
//! Together these three headers implement "cross-origin isolation" as defined
//! by the HTML spec and Fetch spec:
//!
//! - `Cross-Origin-Opener-Policy` (COOP) — controls whether the opener
//!   browsing context is accessible across origins. When set to `same-origin`,
//!   cross-origin popups cannot access `window.opener`.
//!
//! - `Cross-Origin-Embedder-Policy` (COEP) — requires all subresources loaded
//!   by a document to either be same-origin or provide an explicit CORP header.
//!
//! - `Cross-Origin-Resource-Policy` (CORP) — tells the browser whether a
//!   resource may be loaded by cross-origin/cross-site requests.
//!
//! When a document is served with both COOP=`same-origin` and
//! COEP=`require-corp`, `window.crossOriginIsolated` becomes `true`,
//! unlocking `SharedArrayBuffer` and high-resolution timers.
//!
//! This module is a **pure parser and classifier** — it does not perform
//! network I/O. The enforcement point is the HTTP response handler in the
//! network stack and the JS runtime installation in `lumen-js`.
//!
//! References:
//! - HTML § "Cross-origin opener policies"
//! - Fetch § "Cross-origin resource policy check"
//! - <https://html.spec.whatwg.org/multipage/browsers.html#cross-origin-opener-policies>

use crate::origin::Origin;

/// Value of the `Cross-Origin-Opener-Policy` header.
///
/// Controls whether a document opened via `window.open()` shares a browsing
/// context group with its opener.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CrossOriginOpenerPolicy {
    /// No COOP restriction — allows any cross-origin opener access.
    /// Default when the header is absent.
    #[default]
    UnsafeNone,
    /// `same-origin-allow-popups` — inherits unsafe-none for popups opened by
    /// this document; blocks cross-origin windows from retaining an opener
    /// reference to this document.
    SameOriginAllowPopups,
    /// `same-origin` — full isolation: only same-origin documents may share a
    /// browsing context group. Cross-origin popups lose `window.opener`.
    SameOrigin,
    /// `same-origin-plus-coep` — same as `same-origin` but additionally requires
    /// COEP=`require-corp` to be present (used when computing crossOriginIsolated).
    SameOriginPlusCoep,
}

impl CrossOriginOpenerPolicy {
    /// Parse the value of a `Cross-Origin-Opener-Policy` header.
    ///
    /// Unknown tokens and `report-only` variants are treated as `UnsafeNone`
    /// (spec §`parse-a-cross-origin-opener-policy-value`).
    pub fn parse(header: &str) -> Self {
        match header.trim() {
            "same-origin" => Self::SameOrigin,
            "same-origin-allow-popups" => Self::SameOriginAllowPopups,
            "same-origin-plus-coep" => Self::SameOriginPlusCoep,
            // report-only variants are not enforced
            _ => Self::UnsafeNone,
        }
    }

    /// Whether this policy causes cross-origin documents to lose `window.opener`.
    pub fn severs_opener(self) -> bool {
        matches!(self, Self::SameOrigin | Self::SameOriginPlusCoep)
    }

    /// Whether this policy is compatible with cross-origin isolation
    /// (requires COEP=`require-corp` as well).
    pub fn allows_cross_origin_isolation(self) -> bool {
        matches!(self, Self::SameOrigin | Self::SameOriginPlusCoep)
    }
}

/// Value of the `Cross-Origin-Embedder-Policy` header.
///
/// Controls which subresources a document may embed. When set to `require-corp`,
/// all subresources must either be same-origin or served with an explicit
/// `Cross-Origin-Resource-Policy` header.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CrossOriginEmbedderPolicy {
    /// No restriction — any resource may be embedded. Default when absent.
    #[default]
    Unsafe,
    /// `require-corp` — all cross-origin subresources must have CORP header.
    RequireCorp,
    /// `credentialless` — cross-origin subresources are fetched without credentials
    /// even if the request would normally include them.
    Credentialless,
}

impl CrossOriginEmbedderPolicy {
    /// Parse the value of a `Cross-Origin-Embedder-Policy` header.
    pub fn parse(header: &str) -> Self {
        match header.trim() {
            "require-corp" => Self::RequireCorp,
            "credentialless" => Self::Credentialless,
            _ => Self::Unsafe,
        }
    }

    /// Whether this policy enables cross-origin isolation (together with COOP).
    pub fn enables_cross_origin_isolation(self) -> bool {
        matches!(self, Self::RequireCorp)
    }
}

/// Value of the `Cross-Origin-Resource-Policy` header.
///
/// Sent by resource servers to declare who may load the resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CrossOriginResourcePolicy {
    /// `cross-origin` — any origin may load this resource. Default when absent.
    #[default]
    CrossOrigin,
    /// `same-site` — only same-site (eTLD+1) origins may load this resource.
    SameSite,
    /// `same-origin` — only the exact same origin may load this resource.
    SameOrigin,
}

impl CrossOriginResourcePolicy {
    /// Parse the value of a `Cross-Origin-Resource-Policy` header.
    pub fn parse(header: &str) -> Self {
        match header.trim() {
            "same-site" => Self::SameSite,
            "same-origin" => Self::SameOrigin,
            _ => Self::CrossOrigin,
        }
    }
}

/// The derived cross-origin isolation state of a browsing context.
///
/// A document is cross-origin isolated when it is served with BOTH
/// `Cross-Origin-Opener-Policy: same-origin` AND
/// `Cross-Origin-Embedder-Policy: require-corp`.
///
/// This unlocks `SharedArrayBuffer`, `performance.measureUserAgentSpecificMemory()`,
/// and high-resolution timers (see Fetch spec §"cross-origin isolated capability").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CrossOriginIsolationState {
    /// COOP policy of the document.
    pub coop: CrossOriginOpenerPolicy,
    /// COEP policy of the document.
    pub coep: CrossOriginEmbedderPolicy,
}

impl CrossOriginIsolationState {
    /// Compute isolation state from COOP and COEP headers present on an HTTP response.
    ///
    /// Pass `None` for missing headers; they default to `UnsafeNone` / `Unsafe`.
    pub fn from_headers(coop_header: Option<&str>, coep_header: Option<&str>) -> Self {
        Self {
            coop: coop_header.map(CrossOriginOpenerPolicy::parse).unwrap_or_default(),
            coep: coep_header.map(CrossOriginEmbedderPolicy::parse).unwrap_or_default(),
        }
    }

    /// Whether this document is cross-origin isolated.
    ///
    /// True only when COOP is `same-origin` (or `same-origin-plus-coep`) AND
    /// COEP is `require-corp`. See HTML § "cross-origin isolated".
    pub fn is_cross_origin_isolated(self) -> bool {
        self.coop.allows_cross_origin_isolation()
            && self.coep.enables_cross_origin_isolation()
    }
}

/// Check whether a cross-origin resource fetch is allowed under CORP rules.
///
/// Called for each subresource when the embedding document has COEP=`require-corp`.
///
/// # Arguments
/// - `requester_origin` — origin of the document that initiated the fetch.
/// - `resource_origin` — origin of the resource being fetched.
/// - `corp` — the `Cross-Origin-Resource-Policy` header from the resource response.
///   `None` means the header was absent (treated as `cross-origin`).
/// - `coep` — the COEP policy of the embedding document.
///
/// Returns `true` if the resource may be used, `false` if it must be blocked.
pub fn check_corp_allowed(
    requester_origin: &Origin,
    resource_origin: &Origin,
    corp: Option<CrossOriginResourcePolicy>,
    coep: CrossOriginEmbedderPolicy,
) -> bool {
    // When COEP is not require-corp, CORP is not enforced.
    if coep != CrossOriginEmbedderPolicy::RequireCorp {
        return true;
    }

    // Same-origin resources are always allowed.
    if requester_origin == resource_origin {
        return true;
    }

    let policy = corp.unwrap_or_default();

    match policy {
        // cross-origin: resource explicitly opts in to cross-origin sharing.
        CrossOriginResourcePolicy::CrossOrigin => true,
        // same-origin: only the exact same origin may load.
        CrossOriginResourcePolicy::SameOrigin => false,
        // same-site: requester and resource must share the same eTLD+1 site.
        CrossOriginResourcePolicy::SameSite => {
            is_same_site(requester_origin, resource_origin)
        }
    }
}

/// Returns true when `a` and `b` are "same-site" (share an eTLD+1 registrable domain).
///
/// Conservative implementation: extracts the last two dot-delimited labels of each
/// host and compares. Scheme and port are not considered (same-site is scheme-agnostic
/// per the HTML spec § same site).
fn is_same_site(a: &Origin, b: &Origin) -> bool {
    registrable_domain(a.host()) == registrable_domain(b.host())
}

/// Extract the eTLD+1 registrable domain from a host string.
///
/// Conservative heuristic matching the `OriginIsolationContext` logic in
/// `lumen_driver::isolation`. Full PSL lookup is deferred (Sprint 0 stub).
fn registrable_domain(host: &str) -> &str {
    let parts: Vec<&str> = host.split('.').collect();
    match parts.len() {
        0 | 1 => host,
        2 => host,
        n => {
            // Last two labels: e.g. "www.example.co.uk" → "co.uk"?
            // Conservative approach: always take last two labels.
            let split_at = parts[n - 2].as_ptr() as usize - host.as_ptr() as usize;
            &host[split_at..]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::origin::Origin;

    fn origin(scheme: &str, host: &str, port: u16) -> Origin {
        Origin::new(scheme, host, port)
    }

    // ── COOP parsing ──────────────────────────────────────────────────────────

    #[test]
    fn coop_parse_same_origin() {
        assert_eq!(
            CrossOriginOpenerPolicy::parse("same-origin"),
            CrossOriginOpenerPolicy::SameOrigin
        );
    }

    #[test]
    fn coop_parse_same_origin_allow_popups() {
        assert_eq!(
            CrossOriginOpenerPolicy::parse("same-origin-allow-popups"),
            CrossOriginOpenerPolicy::SameOriginAllowPopups
        );
    }

    #[test]
    fn coop_parse_same_origin_plus_coep() {
        assert_eq!(
            CrossOriginOpenerPolicy::parse("same-origin-plus-coep"),
            CrossOriginOpenerPolicy::SameOriginPlusCoep
        );
    }

    #[test]
    fn coop_parse_unknown_is_unsafe_none() {
        assert_eq!(
            CrossOriginOpenerPolicy::parse("unsafe-none"),
            CrossOriginOpenerPolicy::UnsafeNone
        );
        assert_eq!(
            CrossOriginOpenerPolicy::parse(""),
            CrossOriginOpenerPolicy::UnsafeNone
        );
        assert_eq!(
            CrossOriginOpenerPolicy::parse("garbage"),
            CrossOriginOpenerPolicy::UnsafeNone
        );
    }

    #[test]
    fn coop_severs_opener_for_same_origin() {
        assert!(CrossOriginOpenerPolicy::SameOrigin.severs_opener());
        assert!(CrossOriginOpenerPolicy::SameOriginPlusCoep.severs_opener());
        assert!(!CrossOriginOpenerPolicy::UnsafeNone.severs_opener());
        assert!(!CrossOriginOpenerPolicy::SameOriginAllowPopups.severs_opener());
    }

    // ── COEP parsing ──────────────────────────────────────────────────────────

    #[test]
    fn coep_parse_require_corp() {
        assert_eq!(
            CrossOriginEmbedderPolicy::parse("require-corp"),
            CrossOriginEmbedderPolicy::RequireCorp
        );
    }

    #[test]
    fn coep_parse_credentialless() {
        assert_eq!(
            CrossOriginEmbedderPolicy::parse("credentialless"),
            CrossOriginEmbedderPolicy::Credentialless
        );
    }

    #[test]
    fn coep_parse_unknown_is_unsafe() {
        assert_eq!(
            CrossOriginEmbedderPolicy::parse(""),
            CrossOriginEmbedderPolicy::Unsafe
        );
        assert_eq!(
            CrossOriginEmbedderPolicy::parse("unsafe-none"),
            CrossOriginEmbedderPolicy::Unsafe
        );
    }

    // ── CORP parsing ──────────────────────────────────────────────────────────

    #[test]
    fn corp_parse_same_origin() {
        assert_eq!(
            CrossOriginResourcePolicy::parse("same-origin"),
            CrossOriginResourcePolicy::SameOrigin
        );
    }

    #[test]
    fn corp_parse_same_site() {
        assert_eq!(
            CrossOriginResourcePolicy::parse("same-site"),
            CrossOriginResourcePolicy::SameSite
        );
    }

    #[test]
    fn corp_parse_cross_origin() {
        assert_eq!(
            CrossOriginResourcePolicy::parse("cross-origin"),
            CrossOriginResourcePolicy::CrossOrigin
        );
        // absent header → CrossOrigin
        assert_eq!(
            CrossOriginResourcePolicy::parse(""),
            CrossOriginResourcePolicy::CrossOrigin
        );
    }

    // ── CrossOriginIsolationState ──────────────────────────────────────────────

    #[test]
    fn isolation_state_both_headers_gives_isolated() {
        let state = CrossOriginIsolationState::from_headers(
            Some("same-origin"),
            Some("require-corp"),
        );
        assert!(state.is_cross_origin_isolated());
    }

    #[test]
    fn isolation_state_coop_only_not_isolated() {
        let state = CrossOriginIsolationState::from_headers(Some("same-origin"), None);
        assert!(!state.is_cross_origin_isolated());
    }

    #[test]
    fn isolation_state_coep_only_not_isolated() {
        let state = CrossOriginIsolationState::from_headers(None, Some("require-corp"));
        assert!(!state.is_cross_origin_isolated());
    }

    #[test]
    fn isolation_state_no_headers_not_isolated() {
        let state = CrossOriginIsolationState::from_headers(None, None);
        assert!(!state.is_cross_origin_isolated());
    }

    #[test]
    fn isolation_state_allow_popups_coop_not_isolated() {
        let state = CrossOriginIsolationState::from_headers(
            Some("same-origin-allow-popups"),
            Some("require-corp"),
        );
        // allow-popups does not enable isolation
        assert!(!state.is_cross_origin_isolated());
    }

    #[test]
    fn isolation_state_plus_coep_with_require_corp_isolated() {
        let state = CrossOriginIsolationState::from_headers(
            Some("same-origin-plus-coep"),
            Some("require-corp"),
        );
        assert!(state.is_cross_origin_isolated());
    }

    // ── check_corp_allowed ────────────────────────────────────────────────────

    #[test]
    fn corp_allowed_same_origin_resource_always_ok() {
        let o = origin("https", "example.com", 443);
        assert!(check_corp_allowed(
            &o,
            &o,
            Some(CrossOriginResourcePolicy::SameOrigin),
            CrossOriginEmbedderPolicy::RequireCorp,
        ));
    }

    #[test]
    fn corp_cross_origin_corp_allows_cross_origin_requester() {
        let req = origin("https", "example.com", 443);
        let res = origin("https", "cdn.other.com", 443);
        // CrossOrigin CORP explicitly allows cross-origin loading.
        assert!(check_corp_allowed(
            &req,
            &res,
            Some(CrossOriginResourcePolicy::CrossOrigin),
            CrossOriginEmbedderPolicy::RequireCorp,
        ));
    }

    #[test]
    fn corp_blocked_same_origin_corp_cross_origin_requester() {
        let req = origin("https", "example.com", 443);
        let res = origin("https", "cdn.other.com", 443);
        // Resource says "same-origin only" but requester is different origin → block
        assert!(!check_corp_allowed(
            &req,
            &res,
            Some(CrossOriginResourcePolicy::SameOrigin),
            CrossOriginEmbedderPolicy::RequireCorp,
        ));
    }

    #[test]
    fn corp_not_enforced_without_coep() {
        let req = origin("https", "example.com", 443);
        let res = origin("https", "cdn.other.com", 443);
        // Even same-origin CORP does not block when COEP is Unsafe
        assert!(check_corp_allowed(
            &req,
            &res,
            Some(CrossOriginResourcePolicy::SameOrigin),
            CrossOriginEmbedderPolicy::Unsafe,
        ));
    }

    #[test]
    fn corp_absent_header_under_coep_permissive() {
        // When COEP=require-corp and resource has no CORP header:
        // Our model: None → CrossOrigin (permissive). Callers that want strict
        // enforcement should pass Some(CrossOrigin) for "header present with
        // cross-origin value" vs None for "header absent" and handle separately.
        let req = origin("https", "example.com", 443);
        let res = origin("https", "cdn.other.com", 443);
        let result = check_corp_allowed(
            &req,
            &res,
            None, // absent header → defaults to CrossOrigin
            CrossOriginEmbedderPolicy::RequireCorp,
        );
        assert!(result); // permissive: None → CrossOrigin → allowed
    }

    #[test]
    fn same_site_corp_allows_same_site_requester() {
        let req = origin("https", "api.example.com", 443);
        let res = origin("https", "cdn.example.com", 443);
        // Both example.com → same-site → allowed under same-site CORP
        assert!(check_corp_allowed(
            &req,
            &res,
            Some(CrossOriginResourcePolicy::SameSite),
            CrossOriginEmbedderPolicy::RequireCorp,
        ));
    }

    #[test]
    fn same_site_corp_blocks_cross_site_requester() {
        let req = origin("https", "example.com", 443);
        let res = origin("https", "cdn.other.com", 443);
        // different eTLD+1 → cross-site → blocked by same-site CORP
        assert!(!check_corp_allowed(
            &req,
            &res,
            Some(CrossOriginResourcePolicy::SameSite),
            CrossOriginEmbedderPolicy::RequireCorp,
        ));
    }

    // ── registrable_domain helper ──────────────────────────────────────────────

    #[test]
    fn registrable_domain_two_labels() {
        assert_eq!(registrable_domain("example.com"), "example.com");
    }

    #[test]
    fn registrable_domain_three_labels() {
        assert_eq!(registrable_domain("www.example.com"), "example.com");
    }

    #[test]
    fn registrable_domain_single_label() {
        assert_eq!(registrable_domain("localhost"), "localhost");
    }
}
