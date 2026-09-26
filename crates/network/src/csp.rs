//! Content Security Policy Level 3 parser.
//! <https://www.w3.org/TR/CSP3/>
//!
//! Parses `Content-Security-Policy` and `Content-Security-Policy-Report-Only`
//! header values into a structured [`CspPolicy`].
//!
//! Phase 0: parsing + data model only.  Enforcement (blocking inline scripts /
//! styles, network requests) is wired by the shell in Phase 1.

use std::collections::HashMap;

use crate::origin::Origin;
use lumen_core::hash::base64_encode;
use lumen_core::url::Url;
use sha2::{Digest, Sha256, Sha384, Sha512};

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

impl HashAlgorithm {
    /// Base64-encoded digest of `body` under this algorithm — CSP3 §8.1
    /// "script-src source hash matching": a `'sha256-…'`/`'sha384-…'`/
    /// `'sha512-…'` source is satisfied when this equals the source's
    /// declared value byte-for-byte (standard base64, matching how the
    /// build tooling that generates these hashes for a page normally
    /// encodes them — a base64url-encoded declared value simply won't
    /// match, same "don't invent equivalence" stance as the rest of this
    /// module).
    pub fn digest_base64(&self, body: &[u8]) -> String {
        match self {
            HashAlgorithm::Sha256 => base64_encode(&Sha256::digest(body)),
            HashAlgorithm::Sha384 => base64_encode(&Sha384::digest(body)),
            HashAlgorithm::Sha512 => base64_encode(&Sha512::digest(body)),
        }
    }
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
    /// Restricts `<script>` elements, inline and external (CSP Level 3 granular split;
    /// falls back to `script-src`).
    ScriptSrcElem,
    /// Restricts inline script event handlers.
    ScriptSrcAttr,
    /// Restricts `<style>` element sources.
    StyleSrc,
    /// Restricts `<style>` and `<link rel=stylesheet>` elements (granular split;
    /// falls back to `style-src`).
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
    /// Restricts `@font-face` `src` sources.
    FontSrc,
    /// Restricts `<frame>` and `<iframe>` sources, and nested browsing
    /// contexts/workers when `frame-src`/`worker-src` is absent (CSP3 §6.4
    /// granular fallback — see [`CspPolicy::fetch_directive_allows_via_child_src`]).
    ChildSrc,
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

/// Parsed value of the `trusted-types` directive (Trusted Types L2 §4.2).
///
/// Grammar: `trusted-types <policy-name>* ['allow-duplicates']? | 'none'` —
/// unlike fetch directives this is not a source list, so it is not modelled
/// as a [`CspDirective`]/[`CspSource`] pair.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TrustedTypesDirective {
    /// `true` for the bare `'none'` keyword — no policy may be created at all,
    /// including `"default"`.
    pub disallow_all: bool,
    /// Explicitly allowed policy names, in source order (empty when
    /// `disallow_all` is set or the directive listed only keywords).
    pub allowed_policy_names: Vec<String>,
    /// `'allow-duplicates'` keyword — permits re-registering an existing name.
    pub allow_duplicates: bool,
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
    /// `true` when `require-trusted-types-for 'script'` is present — the only
    /// sink group Trusted Types L2 defines; other tokens in the directive are
    /// ignored per CSP3 §2.3 unrecognised-token handling.
    pub require_trusted_types_for_script: bool,
    /// Parsed `trusted-types` directive, if present.
    pub trusted_types: Option<TrustedTypesDirective>,
    /// The header/`<meta>` text this policy was parsed from, verbatim — CSP3
    /// §7.8's `SecurityPolicyViolationEvent.originalPolicy` names the text of
    /// the ONE policy that was violated, not every policy the document
    /// declared (GAP-CSPENF срез 56, `bugs/BUG-811-FIXED.md`).
    pub raw: String,
}

impl CspPolicy {
    /// Returns `true` if no directives or flags are set.
    pub fn is_empty(&self) -> bool {
        self.directives.is_empty()
            && self.report_uri.is_empty()
            && self.report_to.is_none()
            && !self.upgrade_insecure_requests
            && !self.block_all_mixed_content
            && !self.require_trusted_types_for_script
            && self.trusted_types.is_none()
    }

    /// Returns the effective source list for `directive` — the first
    /// directive of its CSP3 §6.8.4 «fallback list» the policy sets:
    /// `script-src-elem`/`script-src-attr` → `script-src` → `default-src`,
    /// `style-src-elem`/`style-src-attr` → `style-src` → `default-src`, any
    /// other fetch directive → `default-src` (BUG-1183: the granular
    /// directives used to fall straight to `default-src`, and the gates asked
    /// for `script-src`/`style-src`, so a `*-src-elem` never decided).
    /// `frame-src`/`worker-src` take the `child-src` step through
    /// [`Self::fetch_directive_allows_via_child_src`] instead.
    ///
    /// Returns `None` only when no directive of the list exists.
    pub fn effective_sources(&self, directive: &CspDirective) -> Option<&Vec<CspSource>> {
        let parent = match directive {
            CspDirective::ScriptSrcElem | CspDirective::ScriptSrcAttr => Some(CspDirective::ScriptSrc),
            CspDirective::StyleSrcElem | CspDirective::StyleSrcAttr => Some(CspDirective::StyleSrc),
            _ => None,
        };
        self.directives
            .get(directive)
            .or_else(|| parent.and_then(|p| self.directives.get(&p)))
            .or_else(|| self.directives.get(&CspDirective::DefaultSrc))
    }

    /// `true` if a fetch of `url` for `directive` is allowed by this policy
    /// (CSP3 §6.7.2.8 "Does url match source list in origin with redirect
    /// count?", simplified — no redirect-count tracking, since a blocked
    /// request never starts and so never redirects).
    ///
    /// No directive and no `default-src` fallback — nothing restricts this
    /// fetch, so it is allowed (the same "absence is not a violation" rule
    /// the shell's `script-src` inline check already uses).
    pub fn fetch_directive_allows(
        &self,
        directive: &CspDirective,
        url: &Url,
        self_origin: Option<&Origin>,
    ) -> bool {
        let Some(sources) = self.effective_sources(directive) else {
            return true;
        };
        sources
            .iter()
            .any(|s| source_matches_url(s, url, self_origin))
    }

    /// `true` if this policy's `script-src-elem` (or `script-src`, or
    /// `default-src`) lets a `<script>` element fetch `url` — CSP3 §6.7.1.1 «Script directives
    /// pre-request check» (BUG-1124). Unlike [`Self::fetch_directive_allows`],
    /// which looks at the URL alone, a script element's request carries its
    /// own cryptographic metadata, and the directive decides on it first:
    ///
    /// 1. the element's `nonce` matches a `'nonce-…'` source → allowed,
    ///    whatever the URL;
    /// 2. the list has hash sources and every hash of the element's
    ///    `integrity` metadata is one of them → allowed;
    /// 3. the list has `'strict-dynamic'` → a parser-inserted script is
    ///    blocked, any other allowed — host sources, schemes and `'self'` are
    ///    never consulted;
    /// 4. otherwise the URL must match the source list, as for every other
    ///    fetch directive.
    pub fn script_element_fetch_allows(
        &self,
        url: &Url,
        self_origin: Option<&Origin>,
        request: &ScriptRequestMetadata<'_>,
    ) -> bool {
        let Some(sources) = self.effective_sources(&CspDirective::ScriptSrcElem) else {
            return true;
        };
        // Step 1.1: «Does nonce match source list?» — a non-empty nonce equal
        // to some nonce-source's base64-value.
        if let Some(nonce) = request.nonce.filter(|n| !n.is_empty())
            && sources.iter().any(|s| matches!(s, CspSource::Nonce(n) if n == nonce))
        {
            return true;
        }
        // Step 1.2: integrity bypass — only when the list names hashes at all.
        if integrity_matches_hash_sources(sources, request.integrity) {
            return true;
        }
        // Step 1.3.
        if sources.contains(&CspSource::StrictDynamic) {
            return !request.parser_inserted;
        }
        // Step 1.4.
        sources.iter().any(|s| source_matches_url(s, url, self_origin))
    }

    /// `true` if this policy's `directive` (or `default-src`) lets an inline
    /// block whose text is `body` and whose `nonce=` attribute is `nonce` run:
    /// any `'unsafe-inline'`, matching nonce or matching hash source admits it,
    /// and a policy with no applicable directive does not restrict it. The one
    /// rule shared by the page's own inline `<script>`/`<style>` (shell
    /// `csp_enforce`) and a `<script>` `document.write()` wrote (BUG-568), so
    /// the two can never judge the same text differently.
    pub fn inline_allows(&self, directive: &CspDirective, nonce: Option<&str>, body: &str) -> bool {
        let Some(sources) = self.effective_sources(directive) else {
            return true;
        };
        sources.iter().any(|s| match s {
            CspSource::UnsafeInline => true,
            CspSource::Nonce(n) => nonce.is_some_and(|actual| actual == n),
            CspSource::Hash { algorithm, value } => algorithm.digest_base64(body.as_bytes()) == *value,
            _ => false,
        })
    }

    /// `true` if this policy's `style-src-elem` (or `style-src`, or
    /// `default-src`) lets a `<link rel=stylesheet>` or an `@import` fetch
    /// `url` — CSP3 `style-src-elem` «Pre-request check» (BUG-1175): a `nonce` matching a
    /// `'nonce-…'` source allows the request whatever the URL, otherwise the
    /// URL must match the source list. Unlike scripts there is no integrity
    /// bypass and no `'strict-dynamic'`.
    pub fn style_element_fetch_allows(&self, url: &Url, self_origin: Option<&Origin>, nonce: Option<&str>) -> bool {
        let Some(sources) = self.effective_sources(&CspDirective::StyleSrcElem) else {
            return true;
        };
        if let Some(nonce) = nonce.filter(|n| !n.is_empty())
            && sources.iter().any(|s| matches!(s, CspSource::Nonce(n) if n == nonce))
        {
            return true;
        }
        sources.iter().any(|s| source_matches_url(s, url, self_origin))
    }

    /// Returns the effective source list for `directive`, falling back to
    /// `child-src` and then `default-src` — the CSP3 §6.4 granular chain
    /// that `frame-src` and `worker-src` get (unlike every other fetch
    /// directive, which falls straight to `default-src` via
    /// [`Self::effective_sources`]).
    fn effective_sources_via_child_src(&self, directive: &CspDirective) -> Option<&Vec<CspSource>> {
        self.directives
            .get(directive)
            .or_else(|| self.directives.get(&CspDirective::ChildSrc))
            .or_else(|| self.directives.get(&CspDirective::DefaultSrc))
    }

    /// Same as [`Self::fetch_directive_allows`], but for `frame-src`/
    /// `worker-src` — the two fetch directives CSP3 §6.4 gives an extra
    /// `child-src` fallback step before `default-src`.
    pub fn fetch_directive_allows_via_child_src(
        &self,
        directive: &CspDirective,
        url: &Url,
        self_origin: Option<&Origin>,
    ) -> bool {
        let Some(sources) = self.effective_sources_via_child_src(directive) else {
            return true;
        };
        sources
            .iter()
            .any(|s| source_matches_url(s, url, self_origin))
    }

    /// `true` if this policy's `frame-ancestors` directive allows the
    /// protected document to be embedded by a frame whose origin is
    /// `ancestor_origin` — CSP3 §6.4.2. Unlike every fetch directive above,
    /// `frame-ancestors` is a navigation directive and does **not** fall
    /// back to `default-src` (CSP3 §6.4): its absence means no restriction.
    /// `self_origin` is the protected document's own origin, matching
    /// `'self'`'s meaning in this directive.
    pub fn frame_ancestor_allowed(
        &self,
        ancestor_origin: &Origin,
        self_origin: Option<&Origin>,
    ) -> bool {
        let Some(sources) = self.directives.get(&CspDirective::FrameAncestors) else {
            return true;
        };
        let Ok(ancestor_url) = Url::parse(&ancestor_origin.serialize()) else {
            return true;
        };
        sources
            .iter()
            .any(|s| source_matches_url(s, &ancestor_url, self_origin))
    }

    /// `true` if this policy's `form-action` directive allows a `<form>`
    /// owned by this document to submit to `action_url` — CSP3 §6.4.3. Like
    /// `frame-ancestors`, `form-action` is a navigation directive and does
    /// **not** fall back to `default-src` (CSP3 §6.4): its absence means no
    /// restriction. `self_origin` is the form-owning document's own origin,
    /// matching `'self'`'s meaning in this directive.
    pub fn form_action_allowed(&self, action_url: &Url, self_origin: Option<&Origin>) -> bool {
        let Some(sources) = self.directives.get(&CspDirective::FormAction) else {
            return true;
        };
        sources
            .iter()
            .any(|s| source_matches_url(s, action_url, self_origin))
    }

    /// `true` if this policy's `base-uri` directive allows a document to set
    /// its base URL to `base_url` via `<base href>` — CSP3 §6.4.1. Like
    /// `frame-ancestors`/`form-action`, `base-uri` is a navigation directive
    /// and does **not** fall back to `default-src` (CSP3 §6.4): its absence
    /// means no restriction. `self_origin` is the document's own origin,
    /// matching `'self'`'s meaning in this directive.
    pub fn base_uri_allowed(&self, base_url: &Url, self_origin: Option<&Origin>) -> bool {
        let Some(sources) = self.directives.get(&CspDirective::BaseUri) else {
            return true;
        };
        sources
            .iter()
            .any(|s| source_matches_url(s, base_url, self_origin))
    }

    /// `true` if this policy's `navigate-to` directive allows this document to
    /// navigate itself (or a context it controls) to `target_url` — CSP3
    /// navigation directive. Like `frame-ancestors`/`form-action`/`base-uri`
    /// it does **not** fall back to `default-src`: its absence means no
    /// restriction. `self_origin` is the navigating document's own origin,
    /// matching `'self'`'s meaning in this directive.
    ///
    /// `form-action` is the narrower sibling that governs form submissions
    /// only; a page that sets both has to satisfy each independently, so this
    /// method deliberately knows nothing about `CspDirective::FormAction`.
    pub fn navigate_to_allowed(&self, target_url: &Url, self_origin: Option<&Origin>) -> bool {
        let Some(sources) = self.directives.get(&CspDirective::NavigateTo) else {
            return true;
        };
        sources
            .iter()
            .any(|s| source_matches_url(s, target_url, self_origin))
    }
}

/// What a `<script>` element's request carries besides its URL — the inputs
/// of CSP3 §6.7.1.1 «Script directives pre-request check» that
/// [`CspPolicy::script_element_fetch_allows`] reads.
#[derive(Debug, Clone, Copy, Default)]
pub struct ScriptRequestMetadata<'a> {
    /// The element's cryptographic nonce (its `nonce` content attribute).
    pub nonce: Option<&'a str>,
    /// The element's `integrity` attribute, raw SRI metadata.
    pub integrity: Option<&'a str>,
    /// `true` for a script the HTML parser inserted, `false` for one a
    /// script created — `'strict-dynamic'` trusts only the latter.
    pub parser_inserted: bool,
}

/// CSP3 §6.7.1.1 step 1.2: `true` if `sources` holds at least one hash source
/// and every hash in the SRI `integrity` metadata (SRI §3.3.2 «parse
/// metadata»: whitespace-separated `alg-value[?options]`, unknown algorithms
/// skipped) names one of them. Empty or absent metadata never bypasses.
fn integrity_matches_hash_sources(sources: &[CspSource], integrity: Option<&str>) -> bool {
    if !sources.iter().any(|s| matches!(s, CspSource::Hash { .. })) {
        return false;
    }
    let mut hashes = integrity
        .unwrap_or("")
        .split_ascii_whitespace()
        .filter_map(|token| {
            let (alg, rest) = token.split_once('-')?;
            let algorithm = match alg.to_ascii_lowercase().as_str() {
                "sha256" => HashAlgorithm::Sha256,
                "sha384" => HashAlgorithm::Sha384,
                "sha512" => HashAlgorithm::Sha512,
                _ => return None,
            };
            let value = rest.split_once('?').map_or(rest, |(v, _)| v);
            Some((algorithm, value))
        })
        .peekable();
    if hashes.peek().is_none() {
        return false;
    }
    hashes.all(|(algorithm, value)| {
        sources.iter().any(|s| {
            matches!(s, CspSource::Hash { algorithm: a, value: v } if *a == algorithm && v == value)
        })
    })
}

/// `true` if `source` (one token of a fetch-directive source list) matches
/// `url`. Keyword sources that gate inline content rather than network
/// requests (`'unsafe-inline'`, nonces, hashes, …) never match a URL.
fn source_matches_url(source: &CspSource, url: &Url, self_origin: Option<&Origin>) -> bool {
    match source {
        CspSource::None => false,
        CspSource::SelfOrigin => self_origin.is_some_and(|origin| {
            Origin::from_url(url).is_ok_and(|target| target.same_origin(origin))
        }),
        CspSource::Scheme(scheme) => url.scheme().eq_ignore_ascii_case(scheme.trim_end_matches(':')),
        CspSource::Url(pattern) => host_source_matches(pattern, url),
        CspSource::UnsafeInline
        | CspSource::UnsafeEval
        | CspSource::StrictDynamic
        | CspSource::UnsafeHashes
        | CspSource::Nonce(_)
        | CspSource::Hash { .. } => false,
    }
}

/// Match a CSP3 host-source expression (§6.7.2.4) against `url`.
///
/// Grammar: `[ scheme "://" ] host [ ":" port ] [ "/" path ]`, `host` is
/// `*` or `[ "*." ] label ( "." label )*`.
///
/// Path is intentionally not matched by this slice: a source with a path
/// component (`https://example.com/scripts/`) is treated as if it named the
/// whole host. That is broader than the spec — a URL outside the path still
/// matches here — never narrower, so it cannot turn an allowed fetch into a
/// blocked one; see `bugs/BUG-811-FIXED.md` GAP-CSPENF срез 4 for the scope
/// note.
fn host_source_matches(pattern: &str, url: &Url) -> bool {
    let (scheme_part, rest) = match pattern.split_once("://") {
        Some((scheme, rest)) => (Some(scheme), rest),
        None => (None, pattern),
    };
    if let Some(scheme) = scheme_part
        && !url.scheme().eq_ignore_ascii_case(scheme)
    {
        return false;
    }
    let host_port = rest.split_once('/').map_or(rest, |(hp, _path)| hp);
    let (host_pattern, port_pattern) = match host_port.split_once(':') {
        Some((h, p)) => (h, Some(p)),
        None => (host_port, None),
    };
    if !host_matches(host_pattern, url.host()) {
        return false;
    }
    if let Some(port_pattern) = port_pattern
        && port_pattern != "*"
    {
        let want: Option<u16> = port_pattern.parse().ok();
        if want != url.port().or_else(|| url.effective_port()) {
            return false;
        }
    }
    true
}

/// `host` matches host-source `pattern`: exact match, `*` (any host), or
/// `*.example.com` (that host or any subdomain, not the bare apex per
/// CSP3 §6.7.2.4).
fn host_matches(pattern: &str, host: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(suffix) = pattern.strip_prefix("*.") {
        return host.len() > suffix.len()
            && host.as_bytes()[host.len() - suffix.len() - 1] == b'.'
            && host[host.len() - suffix.len()..].eq_ignore_ascii_case(suffix);
    }
    pattern.eq_ignore_ascii_case(host)
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
    let mut policy = CspPolicy {
        raw: header.to_owned(),
        ..CspPolicy::default()
    };
    parse_into(&mut policy, header);
    policy
}

/// Parse a report-only variant of the CSP header.
pub fn parse_csp_report_only_header(header: &str) -> CspPolicy {
    let mut policy = CspPolicy {
        report_only: true,
        raw: header.to_owned(),
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
            "require-trusted-types-for" => {
                if tokens.any(|t| t.eq_ignore_ascii_case("'script'")) {
                    policy.require_trusted_types_for_script = true;
                }
            }
            "trusted-types" => {
                let mut directive = TrustedTypesDirective::default();
                for token in tokens {
                    match token {
                        "'none'" => directive.disallow_all = true,
                        "'allow-duplicates'" => directive.allow_duplicates = true,
                        name => directive.allowed_policy_names.push(name.to_string()),
                    }
                }
                policy.trusted_types = Some(directive);
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
                    "font-src" => CspDirective::FontSrc,
                    "child-src" => CspDirective::ChildSrc,
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
    fn parse_require_trusted_types_for_script() {
        let p = parse_csp_header("require-trusted-types-for 'script'");
        assert!(p.require_trusted_types_for_script);
    }

    #[test]
    fn require_trusted_types_for_ignores_unknown_sink_group() {
        // Spec defines only 'script' — an unrecognised token must not set the flag.
        let p = parse_csp_header("require-trusted-types-for 'style'");
        assert!(!p.require_trusted_types_for_script);
    }

    #[test]
    fn parse_trusted_types_none() {
        let p = parse_csp_header("trusted-types 'none'");
        let tt = p.trusted_types.unwrap();
        assert!(tt.disallow_all);
        assert!(tt.allowed_policy_names.is_empty());
    }

    #[test]
    fn parse_trusted_types_allowed_names() {
        let p = parse_csp_header("trusted-types default my-policy 'allow-duplicates'");
        let tt = p.trusted_types.unwrap();
        assert!(!tt.disallow_all);
        assert!(tt.allow_duplicates);
        assert_eq!(
            tt.allowed_policy_names,
            vec!["default".to_string(), "my-policy".to_string()]
        );
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

    fn img_url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn no_img_src_directive_allows_anything() {
        let p = parse_csp_header("script-src 'self'");
        assert!(p.fetch_directive_allows(&CspDirective::ImgSrc, &img_url("https://evil.example/x.png"), None));
    }

    #[test]
    fn img_src_none_blocks_everything() {
        let p = parse_csp_header("img-src 'none'");
        assert!(!p.fetch_directive_allows(&CspDirective::ImgSrc, &img_url("https://example.com/x.png"), None));
    }

    #[test]
    fn img_src_self_matches_same_origin() {
        let p = parse_csp_header("img-src 'self'");
        let origin = Origin::from_url(&img_url("https://example.com/")).unwrap();
        assert!(p.fetch_directive_allows(&CspDirective::ImgSrc, &img_url("https://example.com/x.png"), Some(&origin)));
        assert!(!p.fetch_directive_allows(&CspDirective::ImgSrc, &img_url("https://evil.example/x.png"), Some(&origin)));
    }

    #[test]
    fn img_src_self_without_document_origin_never_matches() {
        // No self_origin known (e.g. a file:// document) — 'self' can't match anything.
        let p = parse_csp_header("img-src 'self'");
        assert!(!p.fetch_directive_allows(&CspDirective::ImgSrc, &img_url("https://example.com/x.png"), None));
    }

    #[test]
    fn img_src_scheme_source() {
        let p = parse_csp_header("img-src https:");
        assert!(p.fetch_directive_allows(&CspDirective::ImgSrc, &img_url("https://cdn.example/x.png"), None));
        assert!(!p.fetch_directive_allows(&CspDirective::ImgSrc, &img_url("http://cdn.example/x.png"), None));
    }

    #[test]
    fn img_src_host_source_exact() {
        let p = parse_csp_header("img-src cdn.example.com");
        assert!(p.fetch_directive_allows(&CspDirective::ImgSrc, &img_url("https://cdn.example.com/x.png"), None));
        assert!(!p.fetch_directive_allows(&CspDirective::ImgSrc, &img_url("https://other.example.com/x.png"), None));
    }

    #[test]
    fn img_src_host_source_wildcard_subdomain() {
        let p = parse_csp_header("img-src *.example.com");
        assert!(p.fetch_directive_allows(&CspDirective::ImgSrc, &img_url("https://cdn.example.com/x.png"), None));
        assert!(!p.fetch_directive_allows(&CspDirective::ImgSrc, &img_url("https://example.com/x.png"), None));
        assert!(!p.fetch_directive_allows(&CspDirective::ImgSrc, &img_url("https://notexample.com/x.png"), None));
    }

    #[test]
    fn img_src_host_source_with_scheme_prefix() {
        let p = parse_csp_header("img-src https://cdn.example.com");
        assert!(p.fetch_directive_allows(&CspDirective::ImgSrc, &img_url("https://cdn.example.com/x.png"), None));
        assert!(!p.fetch_directive_allows(&CspDirective::ImgSrc, &img_url("http://cdn.example.com/x.png"), None));
    }

    #[test]
    fn img_src_host_source_with_port() {
        let p = parse_csp_header("img-src cdn.example.com:8443");
        assert!(p.fetch_directive_allows(&CspDirective::ImgSrc, &img_url("https://cdn.example.com:8443/x.png"), None));
        assert!(!p.fetch_directive_allows(&CspDirective::ImgSrc, &img_url("https://cdn.example.com:9000/x.png"), None));
    }

    #[test]
    fn img_src_default_src_fallback() {
        let p = parse_csp_header("default-src 'none'");
        assert!(!p.fetch_directive_allows(&CspDirective::ImgSrc, &img_url("https://example.com/x.png"), None));
    }

    #[test]
    fn img_src_wildcard_host_allows_any() {
        let p = parse_csp_header("img-src *");
        assert!(p.fetch_directive_allows(&CspDirective::ImgSrc, &img_url("https://anything.example/x.png"), None));
    }

    // ── GAP-CSPENF срез 26: `child-src` directive + granular fallback ──────

    #[test]
    fn child_src_directive_parses() {
        let p = parse_csp_header("child-src example.com");
        assert!(p.directives.contains_key(&CspDirective::ChildSrc));
    }

    #[test]
    fn frame_src_via_child_src_falls_back_to_child_src_before_default_src() {
        let p = parse_csp_header("default-src 'none'; child-src example.com");
        assert!(p.fetch_directive_allows_via_child_src(
            &CspDirective::FrameSrc,
            &img_url("https://example.com/frame.html"),
            None
        ));
        assert!(!p.fetch_directive_allows_via_child_src(
            &CspDirective::FrameSrc,
            &img_url("https://other.example/frame.html"),
            None
        ));
    }

    #[test]
    fn frame_src_via_child_src_prefers_its_own_directive_over_child_src() {
        let p = parse_csp_header("frame-src example.com; child-src other.example");
        assert!(p.fetch_directive_allows_via_child_src(
            &CspDirective::FrameSrc,
            &img_url("https://example.com/frame.html"),
            None
        ));
        assert!(!p.fetch_directive_allows_via_child_src(
            &CspDirective::FrameSrc,
            &img_url("https://other.example/frame.html"),
            None
        ));
    }

    #[test]
    fn frame_src_via_child_src_falls_back_to_default_src_without_child_src() {
        let p = parse_csp_header("default-src 'none'");
        assert!(!p.fetch_directive_allows_via_child_src(
            &CspDirective::FrameSrc,
            &img_url("https://example.com/frame.html"),
            None
        ));
    }

    #[test]
    fn no_frame_src_child_src_or_default_src_allows_anything() {
        let p = parse_csp_header("script-src 'self'");
        assert!(p.fetch_directive_allows_via_child_src(
            &CspDirective::FrameSrc,
            &img_url("https://anything.example/frame.html"),
            None
        ));
    }

    // ── GAP-CSPENF срез 27: `frame-ancestors` enforcement ───────────────────

    fn origin(s: &str) -> Origin {
        Origin::from_url(&img_url(s)).unwrap()
    }

    #[test]
    fn frame_ancestors_allows_listed_host() {
        let p = parse_csp_header("frame-ancestors example.com");
        assert!(p.frame_ancestor_allowed(&origin("https://example.com/"), None));
        assert!(!p.frame_ancestor_allowed(&origin("https://other.example/"), None));
    }

    #[test]
    fn frame_ancestors_none_blocks_every_ancestor() {
        let p = parse_csp_header("frame-ancestors 'none'");
        assert!(!p.frame_ancestor_allowed(&origin("https://example.com/"), None));
    }

    #[test]
    fn frame_ancestors_self_matches_protected_documents_own_origin() {
        let p = parse_csp_header("frame-ancestors 'self'");
        let doc_origin = origin("https://example.com/");
        assert!(p.frame_ancestor_allowed(&doc_origin, Some(&doc_origin)));
        assert!(!p.frame_ancestor_allowed(&origin("https://other.example/"), Some(&doc_origin)));
    }

    #[test]
    fn frame_ancestors_absent_does_not_fall_back_to_default_src() {
        // CSP3 §6.4: navigation directives (frame-ancestors, sandbox) never
        // inherit default-src — unlike every fetch directive above.
        let p = parse_csp_header("default-src 'none'");
        assert!(p.frame_ancestor_allowed(&origin("https://anything.example/"), None));
    }

    // ── GAP-CSPENF срез 29: `form-action` enforcement ───────────────────────

    #[test]
    fn form_action_allows_listed_host() {
        let p = parse_csp_header("form-action example.com");
        assert!(p.form_action_allowed(&img_url("https://example.com/submit"), None));
        assert!(!p.form_action_allowed(&img_url("https://other.example/submit"), None));
    }

    #[test]
    fn form_action_none_blocks_every_target() {
        let p = parse_csp_header("form-action 'none'");
        assert!(!p.form_action_allowed(&img_url("https://example.com/submit"), None));
    }

    #[test]
    fn form_action_self_matches_form_owning_documents_own_origin() {
        let p = parse_csp_header("form-action 'self'");
        let doc_origin = origin("https://example.com/");
        assert!(p.form_action_allowed(&img_url("https://example.com/submit"), Some(&doc_origin)));
        assert!(!p.form_action_allowed(&img_url("https://other.example/submit"), Some(&doc_origin)));
    }

    #[test]
    fn form_action_absent_does_not_fall_back_to_default_src() {
        // CSP3 §6.4: navigation directives (form-action, frame-ancestors,
        // sandbox) never inherit default-src — unlike every fetch directive
        // above.
        let p = parse_csp_header("default-src 'none'");
        assert!(p.form_action_allowed(&img_url("https://anything.example/submit"), None));
    }

    // ── GAP-CSPENF срез 32: `base-uri` enforcement ───────────────────────

    #[test]
    fn base_uri_allows_listed_host() {
        let p = parse_csp_header("base-uri example.com");
        assert!(p.base_uri_allowed(&img_url("https://example.com/base/"), None));
        assert!(!p.base_uri_allowed(&img_url("https://other.example/base/"), None));
    }

    #[test]
    fn base_uri_none_blocks_every_target() {
        let p = parse_csp_header("base-uri 'none'");
        assert!(!p.base_uri_allowed(&img_url("https://example.com/base/"), None));
    }

    #[test]
    fn base_uri_self_matches_documents_own_origin() {
        let p = parse_csp_header("base-uri 'self'");
        let doc_origin = origin("https://example.com/");
        assert!(p.base_uri_allowed(&img_url("https://example.com/base/"), Some(&doc_origin)));
        assert!(!p.base_uri_allowed(&img_url("https://other.example/base/"), Some(&doc_origin)));
    }

    #[test]
    fn base_uri_absent_does_not_fall_back_to_default_src() {
        // CSP3 §6.4: navigation directives (base-uri, form-action,
        // frame-ancestors, sandbox) never inherit default-src — unlike every
        // fetch directive above.
        let p = parse_csp_header("default-src 'none'");
        assert!(p.base_uri_allowed(&img_url("https://anything.example/base/"), None));
    }

    // ── GAP-CSPENF срез 33: `navigate-to` enforcement ────────────────────

    #[test]
    fn navigate_to_allows_listed_host() {
        let p = parse_csp_header("navigate-to example.com");
        assert!(p.navigate_to_allowed(&img_url("https://example.com/next"), None));
        assert!(!p.navigate_to_allowed(&img_url("https://other.example/next"), None));
    }

    #[test]
    fn navigate_to_none_blocks_every_target() {
        let p = parse_csp_header("navigate-to 'none'");
        assert!(!p.navigate_to_allowed(&img_url("https://example.com/next"), None));
    }

    #[test]
    fn navigate_to_self_matches_documents_own_origin() {
        let p = parse_csp_header("navigate-to 'self'");
        let doc_origin = origin("https://example.com/");
        assert!(p.navigate_to_allowed(&img_url("https://example.com/next"), Some(&doc_origin)));
        assert!(!p.navigate_to_allowed(&img_url("https://other.example/next"), Some(&doc_origin)));
    }

    #[test]
    fn navigate_to_absent_does_not_fall_back_to_default_src() {
        // CSP3 §6.4: navigation directives (navigate-to, base-uri,
        // form-action, frame-ancestors, sandbox) never inherit default-src —
        // unlike every fetch directive above.
        let p = parse_csp_header("default-src 'none'");
        assert!(p.navigate_to_allowed(&img_url("https://anything.example/next"), None));
    }

    #[test]
    fn navigate_to_is_independent_of_form_action() {
        // A page may set one without the other; neither substitutes for the
        // other, so a `form-action`-only policy leaves link navigation free
        // and a `navigate-to`-only policy leaves submissions free.
        let only_form = parse_csp_header("form-action 'none'");
        assert!(only_form.navigate_to_allowed(&img_url("https://example.com/next"), None));
        let only_nav = parse_csp_header("navigate-to 'none'");
        assert!(only_nav.form_action_allowed(&img_url("https://example.com/submit"), None));
    }

    // ── BUG-1124: CSP3 §6.7.1.1 script directives pre-request check ───────

    fn script_req<'a>(nonce: Option<&'a str>, integrity: Option<&'a str>, parser_inserted: bool) -> ScriptRequestMetadata<'a> {
        ScriptRequestMetadata { nonce, integrity, parser_inserted }
    }

    #[test]
    fn script_element_matching_nonce_allows_any_url() {
        let p = parse_csp_header("script-src 'nonce-abc'");
        let url = img_url("https://cdn.other.example/ext.js");
        assert!(p.script_element_fetch_allows(&url, None, &script_req(Some("abc"), None, true)));
        assert!(!p.script_element_fetch_allows(&url, None, &script_req(Some("abd"), None, true)));
        assert!(!p.script_element_fetch_allows(&url, None, &script_req(None, None, true)));
        // An empty nonce never matches, even against an empty-looking source.
        assert!(!p.script_element_fetch_allows(&url, None, &script_req(Some(""), None, true)));
    }

    #[test]
    fn script_element_nonce_bypasses_strict_dynamic_for_parser_inserted() {
        let p = parse_csp_header("script-src 'nonce-abc' 'strict-dynamic'");
        let url = img_url("https://example.com/ext.js");
        assert!(p.script_element_fetch_allows(&url, None, &script_req(Some("abc"), None, true)));
    }

    #[test]
    fn strict_dynamic_ignores_host_sources_for_parser_inserted() {
        // 'strict-dynamic' drops host/scheme/'self' matching: a parser-inserted
        // script without a nonce is blocked even from an allowed host, one a
        // script inserted is allowed from anywhere.
        let p = parse_csp_header("script-src 'nonce-abc' 'strict-dynamic' https: 'self'");
        let doc_origin = origin("https://example.com/");
        let url = img_url("https://example.com/dyn.js");
        assert!(!p.script_element_fetch_allows(&url, Some(&doc_origin), &script_req(None, None, true)));
        assert!(p.script_element_fetch_allows(&url, Some(&doc_origin), &script_req(None, None, false)));
    }

    #[test]
    fn script_element_without_strict_dynamic_falls_back_to_url() {
        let p = parse_csp_header("script-src 'nonce-abc' cdn.example.com");
        assert!(p.script_element_fetch_allows(&img_url("https://cdn.example.com/a.js"), None, &script_req(None, None, true)));
        assert!(!p.script_element_fetch_allows(&img_url("https://evil.example/a.js"), None, &script_req(None, None, false)));
    }

    #[test]
    fn script_element_integrity_bypass_needs_every_hash_listed() {
        let p = parse_csp_header("script-src 'sha256-AAA' 'sha384-BBB'");
        let url = img_url("https://evil.example/a.js");
        assert!(p.script_element_fetch_allows(&url, None, &script_req(None, Some("sha256-AAA"), true)));
        assert!(p.script_element_fetch_allows(&url, None, &script_req(None, Some("sha256-AAA?opt sha384-BBB"), true)));
        assert!(!p.script_element_fetch_allows(&url, None, &script_req(None, Some("sha256-AAA sha384-CCC"), true)));
        assert!(!p.script_element_fetch_allows(&url, None, &script_req(None, Some(""), true)));
        // Unknown algorithms are skipped, not failed; with nothing left there
        // is no bypass.
        assert!(!p.script_element_fetch_allows(&url, None, &script_req(None, Some("md5-AAA"), true)));
    }

    #[test]
    fn script_element_integrity_without_hash_sources_is_not_a_bypass() {
        let p = parse_csp_header("script-src cdn.example.com");
        let url = img_url("https://evil.example/a.js");
        assert!(!p.script_element_fetch_allows(&url, None, &script_req(None, Some("sha256-AAA"), true)));
    }

    #[test]
    fn script_element_falls_back_to_default_src() {
        let p = parse_csp_header("default-src 'nonce-abc'");
        let url = img_url("https://example.com/a.js");
        assert!(p.script_element_fetch_allows(&url, None, &script_req(Some("abc"), None, true)));
        assert!(!p.script_element_fetch_allows(&url, None, &script_req(None, None, true)));
        let none = parse_csp_header("img-src 'none'");
        assert!(none.script_element_fetch_allows(&url, None, &script_req(None, None, true)));
    }

    #[test]
    fn style_element_nonce_or_url_allows() {
        let p = parse_csp_header("style-src 'nonce-abc' cdn.example.com");
        let other = img_url("https://evil.example/a.css");
        assert!(p.style_element_fetch_allows(&other, None, Some("abc")));
        assert!(!p.style_element_fetch_allows(&other, None, Some("xyz")));
        assert!(!p.style_element_fetch_allows(&other, None, None));
        assert!(p.style_element_fetch_allows(&img_url("https://cdn.example.com/a.css"), None, None));
        // `default-src` is the fallback; a policy with neither never blocks.
        let d = parse_csp_header("default-src 'none'");
        assert!(!d.style_element_fetch_allows(&other, None, None));
        let none = parse_csp_header("script-src 'none'");
        assert!(none.style_element_fetch_allows(&other, None, None));
    }

    /// BUG-1183: CSP3 §6.8.4 fallback lists — `*-src-elem`/`*-src-attr` step
    /// through `script-src`/`style-src` before `default-src`, and a set
    /// granular directive wins over its parent.
    #[test]
    fn granular_directives_follow_csp3_fallback_list() {
        let p = parse_csp_header("default-src 'none'; script-src 'self'; style-src-elem 'unsafe-inline'");
        let script_src = p.directives.get(&CspDirective::ScriptSrc);
        assert_eq!(p.effective_sources(&CspDirective::ScriptSrcElem), script_src);
        assert_eq!(p.effective_sources(&CspDirective::ScriptSrcAttr), script_src);
        assert_eq!(
            p.effective_sources(&CspDirective::StyleSrcElem),
            p.directives.get(&CspDirective::StyleSrcElem)
        );
        // `style-src-attr` does not borrow its sibling `style-src-elem`.
        assert_eq!(
            p.effective_sources(&CspDirective::StyleSrcAttr),
            p.directives.get(&CspDirective::DefaultSrc)
        );
        assert!(parse_csp_header("img-src 'none'").effective_sources(&CspDirective::ScriptSrcElem).is_none());
    }

    /// BUG-1183: the element pre-request checks read `script-src-elem`/
    /// `style-src-elem` — Chrome blocks under `style-src-elem 'none'` and
    /// allows `style-src 'none'; style-src-elem 'self'` for a same-origin sheet.
    #[test]
    fn element_checks_read_elem_directives() {
        let doc_origin = origin("https://example.com/");
        let own = img_url("https://example.com/a.css");
        let elem_none = parse_csp_header("style-src-elem 'none'");
        assert!(!elem_none.style_element_fetch_allows(&own, Some(&doc_origin), None));
        let elem_self = parse_csp_header("style-src 'none'; style-src-elem 'self'");
        assert!(elem_self.style_element_fetch_allows(&own, Some(&doc_origin), None));

        let js = img_url("https://example.com/a.js");
        let script_none = parse_csp_header("script-src-elem 'none'");
        assert!(!script_none.script_element_fetch_allows(&js, Some(&doc_origin), &script_req(None, None, true)));
        let script_self = parse_csp_header("script-src 'none'; script-src-elem 'self'");
        assert!(script_self.script_element_fetch_allows(&js, Some(&doc_origin), &script_req(None, None, true)));
    }
}
