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
//! - `$option,...`                — options stripped; per-type filtering is Phase 2

use std::collections::{HashMap, HashSet};

use regex::Regex;

use lumen_core::url::Url;
use lumen_core::ext::RequestFilter;

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

/// A single parsed filter rule.
#[derive(Debug, Clone)]
struct FilterEntry {
    kind: MatchKind,
    /// Human-readable label for the `reason` field in `RequestBlocked` events.
    reason: String,
}

impl FilterEntry {
    /// Returns `true` if this entry matches `url`.
    fn matches(&self, url: &Url) -> bool {
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

        // Strip options after `$` (except `$` inside a URL — detect by `||` anchor).
        let pattern = strip_options(pattern);

        // Regex rule: `/pattern/` — matches full URL against the regex.
        if let Some(inner) = pattern.strip_prefix('/').and_then(|s| s.strip_suffix('/')) {
            if !inner.is_empty() {
                if let Ok(re) = Regex::new(inner) {
                    let entry = FilterEntry { kind: MatchKind::Regex(re), reason: "easylist".into() };
                    if is_exception {
                        // Regex exceptions are stored in global_block as deny; for simplicity we
                        // skip regex exception support in Phase 1 (rare in real lists).
                    } else {
                        self.global_block.push(entry);
                        self.rule_count += 1;
                    }
                }
            }
            return;
        }

        if let Some(rest) = pattern.strip_prefix("||") {
            self.parse_domain_rule(rest, is_exception);
        } else if pattern.starts_with('|') {
            // Exact prefix rule: `|https://...`
            let url_str = pattern.trim_start_matches('|').trim_end_matches('|').to_string();
            if !url_str.is_empty() {
                let entry = FilterEntry {
                    kind: MatchKind::ExactPrefix(url_str),
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
    /// - `tracker.com^$third-party`→ same (options already stripped)
    fn parse_domain_rule(&mut self, rest: &str, is_exception: bool) {
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
        let entry = FilterEntry { kind, reason: "easylist".into() };

        if is_exception {
            self.allow.entry(host).or_default().push(entry);
        } else {
            self.block.entry(host).or_default().push(entry);
            self.rule_count += 1;
        }
    }

    /// Check if `url` matches any block rule (before exception check).
    fn is_blocked_raw(&self, url: &Url) -> Option<&str> {
        // Walk host hierarchy.
        let host = url.host().to_lowercase();
        if let Some(reason) = self.check_host_rules(&host, url) {
            return Some(reason);
        }
        // Global (substring / exact-prefix) rules.
        for entry in &self.global_block {
            if entry.matches(url) {
                return Some(&entry.reason);
            }
        }
        None
    }

    fn check_host_rules<'a>(&'a self, host: &str, url: &Url) -> Option<&'a str> {
        // Exact host match.
        if let Some(entries) = self.block.get(host) {
            for e in entries {
                if e.matches(url) {
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
                    if e.matches(url) {
                        return Some(&e.reason);
                    }
                }
            }
        }
        None
    }

    /// Check if `url` is covered by an exception rule.
    fn is_allowed(&self, url: &Url) -> bool {
        let host = url.host().to_lowercase();
        // Host-indexed exceptions.
        if let Some(entries) = self.allow.get(&host) {
            for e in entries {
                if e.matches(url) {
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
                    if e.matches(url) {
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
        if let Some(reason) = self.is_blocked_raw(url)
            && !self.is_allowed(url)
        {
            return Some(reason.to_string());
        }
        None
    }
}

// ── Helpers ────────────────────────────────────────────────────────────────

/// Strip Adblock Plus option string after `$`, guarding against bare `$` in URLs.
///
/// A `$` is treated as an option separator only when it comes after a non-URL
/// character (i.e., not inside `http://` or `||host...`).  For simplicity we
/// skip options for rules that do not start with `||` — those are handled
/// before this function is called.
fn strip_options(pattern: &str) -> &str {
    // Only strip if the `$` is outside the domain/path portion.
    // Strategy: find the last `$`; if it's not followed by `/` or `://` it's options.
    if let Some(pos) = pattern.rfind('$') {
        let after = &pattern[pos + 1..];
        // If after the `$` we see typical option keywords, strip them.
        if !after.is_empty()
            && !after.starts_with('/')
            && !after.starts_with("//")
            && after.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '~' || c == ',' || c == '!')
        {
            return &pattern[..pos];
        }
    }
    pattern
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

    // ── Option stripping ──────────────────────────────────────────────────

    #[test]
    fn option_third_party_stripped() {
        let f = filter("||adserver.net^$third-party");
        assert!(f.should_block(&url("https://adserver.net/ad.js")).is_some());
    }

    #[test]
    fn option_script_stripped() {
        let f = filter("||adserver.net^$script,third-party");
        assert!(f.should_block(&url("https://adserver.net/track.js")).is_some());
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
