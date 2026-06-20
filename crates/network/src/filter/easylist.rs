//! EasyList / Adblock Plus network filter parser and matcher.
//!
//! Implements `RequestFilter` via parsed filter rules in EasyList format
//! (the format used by EasyList, EasyPrivacy, uBlock Origin, Brave, etc.).
//!
//! **Scope:** network-layer filters only (patterns that start with `||`, `|`,
//! or plain text). Cosmetic/element-hiding rules (`##`, `#@#`) are ignored.
//!
//! **Supported syntax:**
//! - `||domain.com^`              — block all requests to domain + subdomains
//! - `||domain.com/path`          — block requests whose path starts with `/path`
//! - `@@||domain.com^`            — exception (whitelist) — cancels a block rule
//! - `|https://example.com/|`     — exact URL prefix match
//! - `keyword`                    — substring match against the full URL string
//! - `/regex/`                    — regex pattern match against the full URL string
//! - `!` / `#`                    — comment lines, ignored
//! - `##` / `#@#` / `#?#` / `#$#`— cosmetic rules, ignored
//! - `$option,...`                — resource-type + party options (Phase 2):
//!   `$script,image,stylesheet,font,xmlhttprequest,subdocument,media,other`
//!   (and `~`-negated forms) restrict a rule to matching request types;
//!   `$third-party` / `$~third-party` (`first-party`) restrict by party.
//!   `domain=` and other modifiers (`important`, `match-case`, `csp=`,
//!   `redirect=`, …) are parsed-and-ignored — the rule keeps its type/party
//!   scope, never narrows on an unmodelled modifier (no over-allow).

use std::collections::{HashMap, HashSet};

use regex::Regex;

use lumen_core::url::Url;
use lumen_core::ext::{RequestContext, RequestFilter, ResourceType};

// ── Internal rule types ────────────────────────────────────────────────────

/// How a filter pattern matches the request URL.
#[derive(Debug, Clone)]
enum MatchKind {
    /// Matches the domain itself and all subdomains (`||domain^`).
    DomainAndSubdomains,
    /// Matches only if the path starts with this prefix (`||domain/prefix`).
    PathPrefix(String),
    /// Substring match against the serialized URL (`keyword`).
    Substring(String),
    /// Exact URL prefix match (`|https://...`).
    ExactPrefix(String),
    /// Regex match against the full URL string (`/pattern/`).
    Regex(Regex),
}

/// Resource-type bitmask: one bit per [`ResourceType`]. A rule's
/// [`RuleOptions::types`] is `Some(mask)` of the types it applies to.
mod rmask {
    pub const SCRIPT: u16 = 1 << 0;
    pub const IMAGE: u16 = 1 << 1;
    pub const STYLESHEET: u16 = 1 << 2;
    pub const FONT: u16 = 1 << 3;
    pub const XHR: u16 = 1 << 4;
    pub const SUBDOC: u16 = 1 << 5;
    pub const MEDIA: u16 = 1 << 6;
    pub const OTHER: u16 = 1 << 7;
    /// Every modelled type.
    pub const ALL: u16 = SCRIPT | IMAGE | STYLESHEET | FONT | XHR | SUBDOC | MEDIA | OTHER;
}

/// Single-bit mask for a concrete request resource type.
fn resource_type_bit(rt: ResourceType) -> u16 {
    match rt {
        ResourceType::Script => rmask::SCRIPT,
        ResourceType::Image => rmask::IMAGE,
        ResourceType::Stylesheet => rmask::STYLESHEET,
        ResourceType::Font => rmask::FONT,
        ResourceType::XmlHttpRequest => rmask::XHR,
        ResourceType::Subdocument => rmask::SUBDOC,
        ResourceType::Media => rmask::MEDIA,
        ResourceType::Other => rmask::OTHER,
    }
}

/// Maps a single EasyList type-option keyword to its mask bit, or `None` if the
/// keyword is not a modelled resource type (party/`domain=`/other modifiers).
fn type_option_bit(key: &str) -> Option<u16> {
    Some(match key {
        "script" => rmask::SCRIPT,
        "image" => rmask::IMAGE,
        "stylesheet" | "css" => rmask::STYLESHEET,
        "font" => rmask::FONT,
        "xmlhttprequest" | "xhr" => rmask::XHR,
        "subdocument" | "frame" => rmask::SUBDOC,
        "media" => rmask::MEDIA,
        "other" | "object" | "object-subrequest" => rmask::OTHER,
        _ => return None,
    })
}

/// Parsed `$`-options constraining when a [`FilterEntry`] applies.
///
/// Both fields default to "applies always": `types == None` matches every
/// resource type, `third_party == None` matches first- and third-party alike.
#[derive(Debug, Clone, Copy)]
struct RuleOptions {
    /// Allowed resource-type mask, or `None` for "any type".
    types: Option<u16>,
    /// `Some(true)` — only third-party requests, `Some(false)` — only
    /// first-party, `None` — either.
    third_party: Option<bool>,
}

impl RuleOptions {
    /// Options that match every request (no `$`-restrictions).
    fn all() -> Self {
        Self { types: None, third_party: None }
    }

    /// Returns `true` if `ctx` satisfies the type and party restrictions.
    ///
    /// Unknown context fields ([`RequestContext::unknown`]) satisfy any
    /// restriction (conservative block) — matching the pre-Phase-2 behaviour
    /// where options were stripped entirely.
    fn matches(&self, ctx: &RequestContext) -> bool {
        if let (Some(mask), Some(rt)) = (self.types, ctx.resource_type)
            && resource_type_bit(rt) & mask == 0
        {
            return false;
        }
        if let (Some(want), Some(tp)) = (self.third_party, ctx.third_party)
            && want != tp
        {
            return false;
        }
        true
    }
}

/// Parse the option string after `$` into a [`RuleOptions`].
///
/// Positive type options form an allow-list (`$script,image` → only those);
/// if only negated types are present, the rule applies to all *except* them
/// (`$~image` → everything but images). `third-party`/`first-party` (and the
/// `3p`/`1p` aliases) set the party restriction. `domain=` and any unmodelled
/// modifier are ignored — never narrowing the rule (avoids silently allowing
/// requests a Phase-1 build would have blocked).
fn parse_options(opts: &str) -> RuleOptions {
    if opts.is_empty() {
        return RuleOptions::all();
    }
    let mut include: u16 = 0;
    let mut exclude: u16 = 0;
    let mut has_pos = false;
    let mut has_neg = false;
    let mut third_party: Option<bool> = None;

    for raw in opts.split(',') {
        let opt = raw.trim();
        if opt.is_empty() {
            continue;
        }
        let (neg, name) = match opt.strip_prefix('~') {
            Some(rest) => (true, rest),
            None => (false, opt),
        };
        // `key=value` modifiers (domain=…, csp=…, redirect=…): only the key
        // matters for classification, and none are modelled types/party.
        let key = name.split('=').next().unwrap_or(name);

        if let Some(bit) = type_option_bit(key) {
            if neg {
                exclude |= bit;
                has_neg = true;
            } else {
                include |= bit;
                has_pos = true;
            }
        } else if key == "third-party" || key == "3p" {
            // `~third-party` ⇒ first-party only.
            third_party = Some(!neg);
        } else if key == "first-party" || key == "1p" {
            // `first-party` ⇒ not third-party; `~first-party` ⇒ third-party.
            third_party = Some(neg);
        }
        // Unmodelled modifier (important, match-case, popup, …): ignored.
    }

    let types = if has_pos {
        Some(include)
    } else if has_neg {
        Some(rmask::ALL & !exclude)
    } else {
        None
    };
    RuleOptions { types, third_party }
}

/// A single parsed filter rule.
#[derive(Debug, Clone)]
struct FilterEntry {
    kind: MatchKind,
    /// Type/party restrictions parsed from the rule's `$`-options.
    options: RuleOptions,
    /// Human-readable label for the `reason` field in `RequestBlocked` events.
    reason: String,
}

impl FilterEntry {
    /// Returns `true` if this entry matches `url` under request `ctx` (both the
    /// URL pattern and the `$`-option restrictions must hold).
    fn matches(&self, url: &Url, ctx: &RequestContext) -> bool {
        self.url_matches(url) && self.options.matches(ctx)
    }

    /// Returns `true` if the URL pattern alone matches (ignores `$`-options).
    fn url_matches(&self, url: &Url) -> bool {
        match &self.kind {
            MatchKind::DomainAndSubdomains => true, // looked up by host — already matching
            MatchKind::PathPrefix(prefix) => url.path().starts_with(prefix.as_str()),
            MatchKind::Substring(sub) => url.as_str().contains(sub.as_str()),
            MatchKind::ExactPrefix(pfx) => url.as_str().starts_with(pfx.as_str()),
            MatchKind::Regex(re) => re.is_match(url.as_str()),
        }
    }
}

// ── EasyListFilter ─────────────────────────────────────────────────────────

/// EasyList-format `RequestFilter` implementation.
///
/// Built via [`EasyListFilter::parse`] from the raw text of any EasyList-compatible
/// filter list. Thread-safe (immutable after construction).
///
/// **Matching strategy:**
/// 1. Extract the hostname from the request URL.
/// 2. Walk up the domain hierarchy (e.g. `cdn.tracker.com` → `tracker.com`) and
///    check if any block rule covers the host.
/// 3. If a block rule matches, check exception rules for a whitelist override.
/// 4. Return `Some(reason)` if blocked, `None` if allowed.
#[derive(Debug, Default)]
pub struct EasyListFilter {
    /// Block rules indexed by lowercase hostname.
    block: HashMap<String, Vec<FilterEntry>>,
    /// Exception rules indexed by lowercase hostname.
    allow: HashMap<String, Vec<FilterEntry>>,
    /// Substring/exact-prefix rules not tied to a specific hostname.
    global_block: Vec<FilterEntry>,
    /// Whitelist rules not tied to a specific hostname.
    global_allow: HashSet<String>,
    /// Total block rules loaded (informational).
    rule_count: usize,
}

impl EasyListFilter {
    /// Parse an EasyList-format text and return a filter.
    ///
    /// Lines that cannot be parsed are silently skipped; the filter degrades
    /// gracefully on unknown syntax.
    pub fn parse(text: &str) -> Self {
        let mut filter = Self::default();
        for line in text.lines() {
            filter.parse_line(line.trim());
        }
        filter
    }

    /// Number of block rules loaded.
    pub fn rule_count(&self) -> usize {
        self.rule_count
    }

    // ── Private helpers ────────────────────────────────────────────────────

    fn parse_line(&mut self, line: &str) {
        // Empty lines and comments.
        if line.is_empty() || line.starts_with('!') || line.starts_with('#') {
            return;
        }
        // Cosmetic/element-hiding rules — not network filters.
        if line.contains("##")
            || line.contains("#@#")
            || line.contains("#?#")
            || line.contains("#$#")
        {
            return;
        }

        let (is_exception, pattern) = if let Some(rest) = line.strip_prefix("@@") {
            (true, rest)
        } else {
            (false, line)
        };

        // Split options after `$` (except `$` inside a URL — detect by `||` anchor).
        let (pattern, opt_str) = split_options(pattern);
        let options = parse_options(opt_str);

        // Regex rule: `/pattern/` — matches full URL against the regex.
        if let Some(inner) = pattern.strip_prefix('/').and_then(|s| s.strip_suffix('/')) {
            if !inner.is_empty()
                && let Ok(re) = Regex::new(inner)
                && !is_exception  // Regex exceptions skipped in Phase 1 (rare in real lists)
            {
                self.global_block.push(FilterEntry { kind: MatchKind::Regex(re), options, reason: "easylist".into() });
                self.rule_count += 1;
            }
            return;
        }

        if let Some(rest) = pattern.strip_prefix("||") {
            self.parse_domain_rule(rest, is_exception, options);
        } else if pattern.starts_with('|') {
            // Exact prefix rule: `|https://...`
            let url_str = pattern.trim_start_matches('|').trim_end_matches('|').to_string();
            if !url_str.is_empty() {
                let entry = FilterEntry {
                    kind: MatchKind::ExactPrefix(url_str),
                    options,
                    reason: "easylist".into(),
                };
                if is_exception {
                    // Store as a global exact allow.
                    // Simple: add the prefix to global_allow set.
                    if let MatchKind::ExactPrefix(pfx) = &entry.kind {
                        self.global_allow.insert(pfx.clone());
                    }
                } else {
                    self.global_block.push(entry);
                    self.rule_count += 1;
                }
            }
        } else {
            // Substring rule.
            let sub = pattern.to_string();
            if sub.len() >= 4 {
                let entry = FilterEntry {
                    kind: MatchKind::Substring(sub),
                    options,
                    reason: "easylist".into(),
                };
                if is_exception {
                    // Substring exceptions are rare; treat as global allow.
                    if let MatchKind::Substring(s) = &entry.kind {
                        self.global_allow.insert(s.clone());
                    }
                } else {
                    self.global_block.push(entry);
                    self.rule_count += 1;
                }
            }
        }
    }

    /// Parse a domain-anchored rule after the `||` prefix has been stripped.
    ///
    /// Examples of `rest`:
    /// - `tracker.com^`            → domain + subdomains block
    /// - `tracker.com/ads/`        → domain + path prefix block
    /// - `tracker.com^$third-party`→ same, with `options.third_party = Some(true)`
    fn parse_domain_rule(&mut self, rest: &str, is_exception: bool, options: RuleOptions) {
        // Split on the first `/` or `^`.
        let (host_part, path_part) = if let Some(slash) = rest.find('/') {
            (&rest[..slash], Some(&rest[slash..]))
        } else {
            // Strip trailing `^` separator.
            let h = rest.trim_end_matches('^');
            (h, None)
        };

        let host = host_part.to_lowercase();
        if host.is_empty() {
            return;
        }

        let kind = match path_part {
            Some(path) => MatchKind::PathPrefix(path.trim_end_matches('^').to_string()),
            None => MatchKind::DomainAndSubdomains,
        };
        let entry = FilterEntry { kind, options, reason: "easylist".into() };

        if is_exception {
            self.allow.entry(host).or_default().push(entry);
        } else {
            self.block.entry(host).or_default().push(entry);
            self.rule_count += 1;
        }
    }

    /// Check if `url` matches any block rule (before exception check).
    fn is_blocked_raw(&self, url: &Url, ctx: &RequestContext) -> Option<&str> {
        // Walk host hierarchy.
        let host = url.host().to_lowercase();
        if let Some(reason) = self.check_host_rules(&host, url, ctx) {
            return Some(reason);
        }
        // Global (substring / exact-prefix) rules.
        for entry in &self.global_block {
            if entry.matches(url, ctx) {
                return Some(&entry.reason);
            }
        }
        None
    }

    fn check_host_rules<'a>(&'a self, host: &str, url: &Url, ctx: &RequestContext) -> Option<&'a str> {
        // Exact host match.
        if let Some(entries) = self.block.get(host) {
            for e in entries {
                if e.matches(url, ctx) {
                    return Some(&e.reason);
                }
            }
        }
        // Walk parent domains: `sub.tracker.com` → try `tracker.com`.
        let mut rest = host;
        while let Some(dot) = rest.find('.') {
            rest = &rest[dot + 1..];
            if (rest.contains('.') || !rest.is_empty())
                && let Some(entries) = self.block.get(rest)
            {
                for e in entries {
                    if e.matches(url, ctx) {
                        return Some(&e.reason);
                    }
                }
            }
        }
        None
    }

    /// Check if `url` is covered by an exception rule.
    fn is_allowed(&self, url: &Url, ctx: &RequestContext) -> bool {
        let host = url.host().to_lowercase();
        // Host-indexed exceptions.
        if let Some(entries) = self.allow.get(&host) {
            for e in entries {
                if e.matches(url, ctx) {
                    return true;
                }
            }
        }
        // Parent domain exceptions.
        let mut rest = host.as_str();
        while let Some(dot) = rest.find('.') {
            rest = &rest[dot + 1..];
            if let Some(entries) = self.allow.get(rest) {
                for e in entries {
                    if e.matches(url, ctx) {
                        return true;
                    }
                }
            }
        }
        // Global allow (exact-prefix / substring exceptions).
        let url_str = url.as_str();
        for allow in &self.global_allow {
            if url_str.starts_with(allow.as_str()) || url_str.contains(allow.as_str()) {
                return true;
            }
        }
        false
    }
}

impl RequestFilter for EasyListFilter {
    fn should_block(&self, url: &Url) -> Option<String> {
        self.should_block_ctx(url, &RequestContext::unknown())
    }

    fn should_block_ctx(&self, url: &Url, ctx: &RequestContext) -> Option<String> {
        if let Some(reason) = self.is_blocked_raw(url, ctx)
            && !self.is_allowed(url, ctx)
        {
            return Some(reason.to_string());
        }
        None
    }
}

// ── Helpers ────────────────────────────────────────────────────────────────

/// Split an Adblock Plus pattern into `(url_pattern, option_string)` at the
/// options-introducing `$`, guarding against a bare `$` inside a URL.
///
/// A `$` is treated as an option separator only when it comes after a non-URL
/// character (i.e., not inside `http://` or `||host...`). When no option `$`
/// is found, the option string is empty.
fn split_options(pattern: &str) -> (&str, &str) {
    // Strategy: find the last `$`; if it's not followed by `/` or `://` it's options.
    if let Some(pos) = pattern.rfind('$') {
        let after = &pattern[pos + 1..];
        if !after.is_empty()
            && !after.starts_with('/')
            && !after.starts_with("//")
            && after.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '~' || c == ',' || c == '!')
        {
            return (&pattern[..pos], after);
        }
    }
    (pattern, "")
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::url::Url;

    fn url(s: &str) -> Url {
        Url::parse(s).expect("valid URL")
    }

    fn filter(rules: &str) -> EasyListFilter {
        EasyListFilter::parse(rules)
    }

    // ── Basic domain block ────────────────────────────────────────────────

    #[test]
    fn domain_block_exact_host() {
        let f = filter("||tracker.com^");
        assert!(f.should_block(&url("https://tracker.com/img/ad.png")).is_some());
    }

    #[test]
    fn domain_block_subdomain() {
        let f = filter("||tracker.com^");
        assert!(f.should_block(&url("https://cdn.tracker.com/js/track.js")).is_some());
    }

    #[test]
    fn domain_block_does_not_match_unrelated() {
        let f = filter("||tracker.com^");
        assert!(f.should_block(&url("https://example.com/page")).is_none());
    }

    #[test]
    fn domain_block_does_not_match_suffix_only() {
        // `tracker.com` should NOT match `nottracker.com`.
        let f = filter("||tracker.com^");
        assert!(f.should_block(&url("https://nottracker.com/")).is_none());
    }

    // ── Path-prefix block ─────────────────────────────────────────────────

    #[test]
    fn path_prefix_block() {
        let f = filter("||cdn.example.com/ads/");
        assert!(f.should_block(&url("https://cdn.example.com/ads/banner.png")).is_some());
    }

    #[test]
    fn path_prefix_no_match_other_path() {
        let f = filter("||cdn.example.com/ads/");
        assert!(f.should_block(&url("https://cdn.example.com/js/app.js")).is_none());
    }

    // ── Exception rules ───────────────────────────────────────────────────

    #[test]
    fn exception_rule_overrides_block() {
        let f = filter("||tracker.com^\n@@||safe.tracker.com^");
        // Blocked:
        assert!(f.should_block(&url("https://tracker.com/ads/")).is_some());
        // Whitelisted:
        assert!(f.should_block(&url("https://safe.tracker.com/api")).is_none());
    }

    #[test]
    fn exception_parent_domain_overrides_subdomain_block() {
        let rules = "||adserver.com^\n@@||ok.adserver.com^";
        let f = filter(rules);
        assert!(f.should_block(&url("https://bad.adserver.com/")).is_some());
        assert!(f.should_block(&url("https://ok.adserver.com/")).is_none());
    }

    // ── Comments ──────────────────────────────────────────────────────────

    #[test]
    fn comment_lines_ignored() {
        let f = filter("! This is a comment\n# Also a comment\n||ads.com^");
        assert!(f.should_block(&url("https://ads.com/x")).is_some());
    }

    // ── Cosmetic rules ────────────────────────────────────────────────────

    #[test]
    fn cosmetic_rules_ignored() {
        let f = filter("example.com##.ad-banner\n||tracker.com^");
        // Only the network rule should be counted.
        assert_eq!(f.rule_count(), 1);
        assert!(f.should_block(&url("https://tracker.com/")).is_some());
    }

    // ── Resource-type & party options (Phase 2) ───────────────────────────

    /// Context with a known resource type and unknown party.
    fn ctx_type(rt: ResourceType) -> RequestContext {
        RequestContext { resource_type: Some(rt), third_party: None }
    }

    #[test]
    fn unknown_context_blocks_typed_rule_like_phase1() {
        // No context (should_block) must still block — conservative, no regression.
        let f = filter("||adserver.net^$third-party");
        assert!(f.should_block(&url("https://adserver.net/ad.js")).is_some());
        let f2 = filter("||adserver.net^$script,third-party");
        assert!(f2.should_block(&url("https://adserver.net/track.js")).is_some());
    }

    #[test]
    fn script_option_blocks_only_scripts() {
        let f = filter("||adserver.net^$script");
        // Script request → blocked.
        assert!(f.should_block_ctx(&url("https://adserver.net/track.js"),
            &ctx_type(ResourceType::Script)).is_some());
        // Image request → allowed (rule restricted to scripts).
        assert!(f.should_block_ctx(&url("https://adserver.net/pixel.png"),
            &ctx_type(ResourceType::Image)).is_none());
    }

    #[test]
    fn multiple_type_options_form_allowlist() {
        let f = filter("||cdn.net^$image,media");
        assert!(f.should_block_ctx(&url("https://cdn.net/a.png"),
            &ctx_type(ResourceType::Image)).is_some());
        assert!(f.should_block_ctx(&url("https://cdn.net/v.mp4"),
            &ctx_type(ResourceType::Media)).is_some());
        assert!(f.should_block_ctx(&url("https://cdn.net/a.js"),
            &ctx_type(ResourceType::Script)).is_none());
    }

    #[test]
    fn negated_type_option_blocks_all_except() {
        let f = filter("||cdn.net^$~image");
        // Everything except image is blocked.
        assert!(f.should_block_ctx(&url("https://cdn.net/a.js"),
            &ctx_type(ResourceType::Script)).is_some());
        // Image is exempt.
        assert!(f.should_block_ctx(&url("https://cdn.net/a.png"),
            &ctx_type(ResourceType::Image)).is_none());
    }

    #[test]
    fn third_party_option_respects_party() {
        let f = filter("||widget.net^$third-party");
        let third = RequestContext { resource_type: None, third_party: Some(true) };
        let first = RequestContext { resource_type: None, third_party: Some(false) };
        assert!(f.should_block_ctx(&url("https://widget.net/x"), &third).is_some());
        assert!(f.should_block_ctx(&url("https://widget.net/x"), &first).is_none());
    }

    #[test]
    fn first_party_option_respects_party() {
        let f = filter("||widget.net^$~third-party");
        let third = RequestContext { resource_type: None, third_party: Some(true) };
        let first = RequestContext { resource_type: None, third_party: Some(false) };
        assert!(f.should_block_ctx(&url("https://widget.net/x"), &first).is_some());
        assert!(f.should_block_ctx(&url("https://widget.net/x"), &third).is_none());
    }

    #[test]
    fn domain_option_ignored_not_narrowing() {
        // domain= is parsed-and-ignored; rule still applies (no over-allow).
        let f = filter("||ads.net^$script,domain=foo.com");
        assert!(f.should_block_ctx(&url("https://ads.net/a.js"),
            &ctx_type(ResourceType::Script)).is_some());
        // But the type restriction is honoured.
        assert!(f.should_block_ctx(&url("https://ads.net/a.png"),
            &ctx_type(ResourceType::Image)).is_none());
    }

    #[test]
    fn unmodelled_option_does_not_narrow() {
        // `$ping` is not modelled → rule applies to all types (no over-allow).
        let f = filter("||ads.net^$ping");
        assert!(f.should_block_ctx(&url("https://ads.net/beacon"),
            &ctx_type(ResourceType::XmlHttpRequest)).is_some());
    }

    #[test]
    fn typed_exception_only_whitelists_matching_type() {
        // Block all, but whitelist images only on this host.
        let f = filter("||cdn.net^\n@@||cdn.net^$image");
        assert!(f.should_block_ctx(&url("https://cdn.net/a.png"),
            &ctx_type(ResourceType::Image)).is_none());
        assert!(f.should_block_ctx(&url("https://cdn.net/a.js"),
            &ctx_type(ResourceType::Script)).is_some());
    }

    #[test]
    fn parse_options_keyword_aliases() {
        assert_eq!(parse_options("css").types, Some(rmask::STYLESHEET));
        assert_eq!(parse_options("xhr").types, Some(rmask::XHR));
        assert_eq!(parse_options("frame").types, Some(rmask::SUBDOC));
        assert!(parse_options("3p").third_party == Some(true));
        assert!(parse_options("1p").third_party == Some(false));
    }

    #[test]
    fn split_options_guards_dollar_in_url() {
        // A `$` mid-URL (followed by a digit) is not an option separator.
        assert_eq!(split_options("|https://x.com/p$1"), ("|https://x.com/p$1", ""));
        assert_eq!(split_options("||ads.net^$script"), ("||ads.net^", "script"));
    }

    // ── Multi-rule list ───────────────────────────────────────────────────

    #[test]
    fn multi_rule_list() {
        let rules = "! EasyList subset\n||adserver.com^\n||tracker.net^\n||analytics.io^";
        let f = filter(rules);
        assert_eq!(f.rule_count(), 3);
        assert!(f.should_block(&url("https://adserver.com/")).is_some());
        assert!(f.should_block(&url("https://tracker.net/js")).is_some());
        assert!(f.should_block(&url("https://analytics.io/pixel")).is_some());
        assert!(f.should_block(&url("https://clean.com/")).is_none());
    }

    // ── Exact-prefix rule ─────────────────────────────────────────────────

    #[test]
    fn exact_prefix_rule() {
        let f = filter("|https://ads.example.com/banner|");
        assert!(f.should_block(&url("https://ads.example.com/banner")).is_some());
        assert!(f.should_block(&url("https://ads.example.com/other")).is_none());
    }

    // ── Regex rules ───────────────────────────────────────────────────────

    #[test]
    fn regex_rule_blocks_matching_url() {
        let f = filter(r"/\.ads\./");
        assert!(f.should_block(&url("https://cdn.ads.example.com/banner.png")).is_some());
    }

    #[test]
    fn regex_rule_does_not_block_non_matching_url() {
        let f = filter(r"/\.ads\./");
        assert!(f.should_block(&url("https://example.com/page")).is_none());
    }

    #[test]
    fn regex_rule_counted_as_block_rule() {
        let f = filter(r"/tracking\.php/");
        assert_eq!(f.rule_count(), 1);
    }

    #[test]
    fn invalid_regex_is_silently_skipped() {
        // `[unclosed` is an invalid regex — should not panic, just skip.
        let f = filter("/[unclosed/\n||tracker.com^");
        assert_eq!(f.rule_count(), 1); // only the domain rule counts
        assert!(f.should_block(&url("https://tracker.com/")).is_some());
    }

    #[test]
    fn regex_rule_matches_query_string() {
        let f = filter(r"/\?.*utm_source=/");
        assert!(f.should_block(&url("https://example.com/page?utm_source=email")).is_some());
        assert!(f.should_block(&url("https://example.com/page")).is_none());
    }

    // ── rule_count ────────────────────────────────────────────────────────

    #[test]
    fn rule_count_correct() {
        let f = filter("||a.com^\n||b.com^\n@@||b.com^\n! comment\n##.ad");
        // 2 block rules, exception + cosmetic don't count.
        assert_eq!(f.rule_count(), 2);
    }
}
