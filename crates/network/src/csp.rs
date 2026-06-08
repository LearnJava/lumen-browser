//! Content Security Policy Level 3 parser.
//! <https://www.w3.org/TR/CSP3/>
//!
//! Parses `Content-Security-Policy` and `Content-Security-Policy-Report-Only`
//! header values into a structured [`CspPolicy`].
//!
//! Phase 0: parsing + data model only.  Enforcement (blocking inline scripts /
//! styles, network requests) is wired by the shell in Phase 1.

use std::collections::HashMap;

/// Hash algorithm used in a CSP hash source expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HashAlgorithm {
    /// SHA-256 (`'sha256-…'`).
    Sha256,
    /// SHA-384 (`'sha384-…'`).
    Sha384,
    /// SHA-512 (`'sha512-…'`).
    Sha512,
}

/// A single source expression from a CSP directive source list.
///
/// Represents one token in a source list such as
/// `'self' 'nonce-abc' https://example.com`.
#[derive(Debug, Clone, PartialEq)]
pub enum CspSource {
    /// `'none'` — no sources allowed for this directive.
    None,
    /// `'self'` — same origin as the document.
    SelfOrigin,
    /// `'unsafe-inline'` — inline scripts / styles are allowed.
    UnsafeInline,
    /// `'unsafe-eval'` — `eval()` and similar constructs are allowed.
    UnsafeEval,
    /// `'strict-dynamic'` — hashes/nonces propagate to dynamically added scripts.
    StrictDynamic,
    /// `'unsafe-hashes'` — hashes may cover inline event handlers and `style` attributes.
    UnsafeHashes,
    /// `'nonce-<base64>'` — specific nonce value.
    Nonce(String),
    /// `'sha256-<b64>'`, `'sha384-<b64>'`, or `'sha512-<b64>'` — hash of inline content.
    Hash {
        /// Which hash function was used.
        algorithm: HashAlgorithm,
        /// Base64-encoded digest value.
        value: String,
    },
    /// Scheme-only source such as `https:` or `data:`.
    Scheme(String),
    /// Full URL or URL pattern (host source).
    Url(String),
}

/// A CSP fetch / navigation directive name.
///
/// Covers all directives defined in CSP Level 3 §6 and §7.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CspDirective {
    // ── Fetch directives ────────────────────────────────────────────────────
    /// Fallback for fetch directives that have no explicit entry.
    DefaultSrc,
    /// Restricts `<script>` element sources.
    ScriptSrc,
    /// Restricts `<script>` element src (CSS Level 3 granular split).
    ScriptSrcElem,
    /// Restricts inline script event handlers.
    ScriptSrcAttr,
    /// Restricts `<style>` element sources.
    StyleSrc,
    /// Restricts `<style>` element (granular split).
    StyleSrcElem,
    /// Restricts inline style attributes.
    StyleSrcAttr,
    /// Restricts `<img>` and CSS image sources.
    ImgSrc,
    /// Restricts fetch, XMLHttpRequest, WebSocket, and EventSource.
    ConnectSrc,
    /// Restricts `<audio>`, `<video>`, and `<track>` sources.
    MediaSrc,
    /// Restricts `<object>` and `<embed>` sources.
    ObjectSrc,
    /// Restricts `<frame>` and `<iframe>` sources.
    FrameSrc,
    /// Restricts Worker, SharedWorker, and ServiceWorker sources.
    WorkerSrc,
    /// Restricts Web App Manifest sources.
    ManifestSrc,
    /// Restricts prefetch and prerender sources (deprecated in CSP3).
    PrefetchSrc,
    // ── Document / navigation directives ────────────────────────────────────
    /// Restricts the `<base>` element `href`.
    BaseUri,
    /// Restricts `<a>`, `form[action]`, and other navigation targets.
    FormAction,
    /// Restricts which pages may embed this document in a frame.
    FrameAncestors,
    /// Restricts navigation targets (CSP3 draft).
    NavigateTo,
    // ── Other ───────────────────────────────────────────────────────────────
    /// `sandbox` directive token list (treated as raw url-like tokens).
    Sandbox,
}

/// A parsed Content Security Policy.
///
/// Produced by [`parse_csp_header`].  Contains all directives from a single
/// CSP header value (multiple headers must be intersected by the caller).
#[derive(Debug, Clone, Default)]
pub struct CspPolicy {
    /// Fetch / navigation directives mapped to their source lists.
    pub directives: HashMap<CspDirective, Vec<CspSource>>,
    /// `report-uri` endpoint URLs (deprecated in CSP3 but still widely used).
    pub report_uri: Vec<String>,
    /// `report-to` group name (CSP3 / Reporting API).
    pub report_to: Option<String>,
    /// Whether `upgrade-insecure-requests` is present.
    pub upgrade_insecure_requests: bool,
    /// Whether `block-all-mixed-content` is present (deprecated but parsed).
    pub block_all_mixed_content: bool,
    /// Whether this policy is report-only (from the `-Report-Only` variant).
    pub report_only: bool,
}

impl CspPolicy {
    /// Returns `true` if no directives or flags are set.
    pub fn is_empty(&self) -> bool {
        self.directives.is_empty()
            && self.report_uri.is_empty()
            && self.report_to.is_none()
            && !self.upgrade_insecure_requests
            && !self.block_all_mixed_content
    }

    /// Returns the effective source list for `directive`, falling back to
    /// `default-src` when the directive is not explicitly set.
    ///
    /// Returns `None` only when neither the directive nor `default-src` exists.
    pub fn effective_sources(&self, directive: &CspDirective) -> Option<&Vec<CspSource>> {
        self.directives
            .get(directive)
            .or_else(|| self.directives.get(&CspDirective::DefaultSrc))
    }
}

/// Parse a `Content-Security-Policy` header value into a [`CspPolicy`].
///
/// Directives are separated by `;`.  Unrecognised directive names are silently
/// ignored, which is the spec-required behaviour (CSP3 §2.3).
///
/// ```
/// use lumen_network::csp::{parse_csp_header, CspDirective, CspSource};
///
/// let policy = parse_csp_header("default-src 'self'; script-src 'self' 'unsafe-inline'");
/// assert!(policy.directives.contains_key(&CspDirective::DefaultSrc));
/// assert!(policy.directives.contains_key(&CspDirective::ScriptSrc));
/// ```
pub fn parse_csp_header(header: &str) -> CspPolicy {
    let mut policy = CspPolicy::default();
    parse_into(&mut policy, header);
    policy
}

/// Parse a report-only variant of the CSP header.
pub fn parse_csp_report_only_header(header: &str) -> CspPolicy {
    let mut policy = CspPolicy {
        report_only: true,
        ..CspPolicy::default()
    };
    parse_into(&mut policy, header);
    policy
}

fn parse_into(policy: &mut CspPolicy, header: &str) {
    for directive_str in header.split(';') {
        let directive_str = directive_str.trim();
        if directive_str.is_empty() {
            continue;
        }

        let mut tokens = directive_str.split_ascii_whitespace();
        let Some(name) = tokens.next() else {
            continue;
        };

        match name.to_ascii_lowercase().as_str() {
            "upgrade-insecure-requests" => {
                policy.upgrade_insecure_requests = true;
            }
            "block-all-mixed-content" => {
                policy.block_all_mixed_content = true;
            }
            "report-uri" => {
                policy.report_uri.extend(tokens.map(str::to_string));
            }
            "report-to" => {
                if let Some(group) = tokens.next() {
                    policy.report_to = Some(group.to_string());
                }
            }
            dir_name => {
                let dir = match dir_name {
                    "default-src" => CspDirective::DefaultSrc,
                    "script-src" => CspDirective::ScriptSrc,
                    "script-src-elem" => CspDirective::ScriptSrcElem,
                    "script-src-attr" => CspDirective::ScriptSrcAttr,
                    "style-src" => CspDirective::StyleSrc,
                    "style-src-elem" => CspDirective::StyleSrcElem,
                    "style-src-attr" => CspDirective::StyleSrcAttr,
                    "img-src" => CspDirective::ImgSrc,
                    "connect-src" => CspDirective::ConnectSrc,
                    "media-src" => CspDirective::MediaSrc,
                    "object-src" => CspDirective::ObjectSrc,
                    "frame-src" => CspDirective::FrameSrc,
                    "worker-src" => CspDirective::WorkerSrc,
                    "manifest-src" => CspDirective::ManifestSrc,
                    "prefetch-src" => CspDirective::PrefetchSrc,
                    "base-uri" => CspDirective::BaseUri,
                    "form-action" => CspDirective::FormAction,
                    "frame-ancestors" => CspDirective::FrameAncestors,
                    "navigate-to" => CspDirective::NavigateTo,
                    "sandbox" => CspDirective::Sandbox,
                    // Unknown directive — skip per CSP3 §2.3.
                    _ => continue,
                };

                let sources: Vec<CspSource> = tokens.map(parse_source).collect();
                policy.directives.insert(dir, sources);
            }
        }
    }
}

/// Parse a single source expression token.
fn parse_source(token: &str) -> CspSource {
    match token.to_ascii_lowercase().as_str() {
        "'none'" => CspSource::None,
        "'self'" => CspSource::SelfOrigin,
        "'unsafe-inline'" => CspSource::UnsafeInline,
        "'unsafe-eval'" => CspSource::UnsafeEval,
        "'strict-dynamic'" => CspSource::StrictDynamic,
        "'unsafe-hashes'" => CspSource::UnsafeHashes,
        _ => {
            // Quoted keyword — check for nonce / hash
            if token.len() >= 9
                && token.starts_with('\'')
                && token.ends_with('\'')
            {
                let inner = &token[1..token.len() - 1];
                if let Some(rest) = inner.strip_prefix("nonce-") {
                    return CspSource::Nonce(rest.to_string());
                }
                if let Some(rest) = inner.strip_prefix("sha256-") {
                    return CspSource::Hash {
                        algorithm: HashAlgorithm::Sha256,
                        value: rest.to_string(),
                    };
                }
                if let Some(rest) = inner.strip_prefix("sha384-") {
                    return CspSource::Hash {
                        algorithm: HashAlgorithm::Sha384,
                        value: rest.to_string(),
                    };
                }
                if let Some(rest) = inner.strip_prefix("sha512-") {
                    return CspSource::Hash {
                        algorithm: HashAlgorithm::Sha512,
                        value: rest.to_string(),
                    };
                }
            }
            // Scheme-only: ends with ':' and no path characters
            if token.ends_with(':') && !token.contains('/') {
                return CspSource::Scheme(token.to_ascii_lowercase());
            }
            // Host source / URL
            CspSource::Url(token.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_self_policy() {
        let p = parse_csp_header("default-src 'self'");
        let sources = p.directives.get(&CspDirective::DefaultSrc).unwrap();
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0], CspSource::SelfOrigin);
    }

    #[test]
    fn parse_multiple_directives() {
        let p = parse_csp_header(
            "default-src 'self'; script-src 'self' 'unsafe-inline'; img-src https:",
        );
        assert!(p.directives.contains_key(&CspDirective::DefaultSrc));
        assert!(p.directives.contains_key(&CspDirective::ScriptSrc));
        assert!(p.directives.contains_key(&CspDirective::ImgSrc));
        let script_src = p.directives.get(&CspDirective::ScriptSrc).unwrap();
        assert_eq!(script_src.len(), 2);
        assert_eq!(script_src[0], CspSource::SelfOrigin);
        assert_eq!(script_src[1], CspSource::UnsafeInline);
    }

    #[test]
    fn parse_nonce_source() {
        let p = parse_csp_header("script-src 'nonce-abc123XY=='");
        let sources = p.directives.get(&CspDirective::ScriptSrc).unwrap();
        assert_eq!(sources[0], CspSource::Nonce("abc123XY==".to_string()));
    }

    #[test]
    fn parse_hash_sources() {
        let p = parse_csp_header(
            "script-src 'sha256-abc' 'sha384-def' 'sha512-ghi'",
        );
        let src = p.directives.get(&CspDirective::ScriptSrc).unwrap();
        assert_eq!(
            src[0],
            CspSource::Hash {
                algorithm: HashAlgorithm::Sha256,
                value: "abc".to_string()
            }
        );
        assert_eq!(
            src[1],
            CspSource::Hash {
                algorithm: HashAlgorithm::Sha384,
                value: "def".to_string()
            }
        );
        assert_eq!(
            src[2],
            CspSource::Hash {
                algorithm: HashAlgorithm::Sha512,
                value: "ghi".to_string()
            }
        );
    }

    #[test]
    fn parse_upgrade_insecure_requests() {
        let p = parse_csp_header("upgrade-insecure-requests");
        assert!(p.upgrade_insecure_requests);
        assert!(p.directives.is_empty());
    }

    #[test]
    fn parse_report_uri() {
        let p = parse_csp_header("default-src 'self'; report-uri /csp-report");
        assert_eq!(p.report_uri, vec!["/csp-report".to_string()]);
    }

    #[test]
    fn parse_report_to() {
        let p = parse_csp_header("default-src 'self'; report-to csp-endpoint");
        assert_eq!(p.report_to, Some("csp-endpoint".to_string()));
    }

    #[test]
    fn parse_none_source() {
        let p = parse_csp_header("object-src 'none'");
        let src = p.directives.get(&CspDirective::ObjectSrc).unwrap();
        assert_eq!(src[0], CspSource::None);
    }

    #[test]
    fn parse_scheme_source() {
        let p = parse_csp_header("img-src https: data:");
        let src = p.directives.get(&CspDirective::ImgSrc).unwrap();
        assert_eq!(src[0], CspSource::Scheme("https:".to_string()));
        assert_eq!(src[1], CspSource::Scheme("data:".to_string()));
    }

    #[test]
    fn effective_sources_falls_back_to_default() {
        let p = parse_csp_header("default-src 'self'");
        let effective = p.effective_sources(&CspDirective::ScriptSrc).unwrap();
        assert_eq!(effective[0], CspSource::SelfOrigin);
    }

    #[test]
    fn unknown_directive_ignored() {
        let p = parse_csp_header("default-src 'self'; unknown-directive foo");
        assert_eq!(p.directives.len(), 1);
    }

    #[test]
    fn report_only_flag() {
        let p = parse_csp_report_only_header("default-src 'self'");
        assert!(p.report_only);
    }
}
