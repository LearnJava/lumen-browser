//! The compat pack (COMPAT-PACK, ADR-034 §4, `docs/research/site-compat/report.md` §7.6 p.4):
//! signed declarative records «root cause × site» from a closed action vocabulary.
//!
//! Data, never code: an unknown field or an unknown action does not parse, so a pack
//! cannot grow behaviour the browser does not already have built in. A pack comes from
//! two places and both go through [`CompatPack::validate`]:
//!
//! * the base copy compiled into the binary ([`CompatPack::base`], `assets/compat_pack.json`,
//!   trusted because it ships with the build, like `hsts_preload.json`), and
//! * a pack fetched later ([`CompatPack::parse_signed`]) — ed25519 over
//!   `lumen-compat-pack-v1 ‖ 0x00 ‖ signing_body`, so a release-manifest signature never
//!   verifies as a pack and vice versa (the shared [`crate::sign_tagged`]).
//!
//! Matching ([`CompatPack::entries_for_host`]) is never broader than eTLD+1: a pattern is a
//! plain lowercase ASCII host that must itself be a registrable domain or deeper (checked
//! against the [`PublicSuffixList`] trait-anchor — `*` and public suffixes are rejected).

use std::collections::HashSet;

use lumen_core::ext::PublicSuffixList;
use serde::{Deserialize, Serialize};

use crate::{ManifestVerifyError, Version, sign_tagged, verify_tagged};

/// Current `format` of the wire type; a pack with another value is rejected.
pub const PACK_FORMAT: u32 = 1;

/// Domain tag of the pack signature (see [`crate::sign_tagged`]).
const PACK_TAG: &str = "lumen-compat-pack-v1";

/// Public keys this build trusts to sign a fetched pack, `(key_id, raw ed25519 key)`.
///
/// Separate from [`crate::TRUSTED_KEYS`] so the pack can be signed on its own cadence by
/// another key. Empty until the owner mints one (`docs/release-signing.md`): a fetched
/// pack is then rejected with [`PackError::Signature`]`(UnknownKeyId)`, never trusted.
pub const PACK_TRUSTED_KEYS: &[(&str, [u8; 32])] = &[];

/// ADR-034 §4: at most this many active `js_transform` records.
pub const MAX_JS_TRANSFORMS: usize = 20;

/// Upper bound on a pack document — it is network input until verified.
pub const MAX_PACK_BYTES: usize = 1 << 20;
const MAX_ENTRIES: usize = 2000;
const MAX_PATTERNS: usize = 16;

/// Hosts that count as bank/payment/gov whatever the record declares: a mislabelled
/// `site_class: general` must not unlock `protection` or `js_transform` there.
const SENSITIVE_DOMAINS: &[&str] = &[
    "sberbank.ru", "sber.ru", "vtb.ru", "alfabank.ru", "tbank.ru", "tinkoff.ru", "gazprombank.ru",
    "raiffeisen.ru", "psbank.ru", "mkb.ru", "sovcombank.ru", "yoomoney.ru", "qiwi.com", "mironline.ru",
    "nspk.ru", "paypal.com", "stripe.com", "gosuslugi.ru", "nalog.gov.ru", "mos.ru", "pfr.gov.ru",
    "sfr.gov.ru",
];

/// What kind of site an entry targets. `Bank`, `Payment` and `Gov` restrict the actions to
/// [`Action::Identity`], [`Action::Css`] and [`Action::KnownIssue`] (ADR-034 §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SiteClass {
    /// Ordinary site: every action allowed.
    #[default]
    General,
    /// Bank.
    Bank,
    /// Payment service or checkout.
    Payment,
    /// Government service.
    Gov,
}

/// Identity profile an [`Action::Identity`] record switches to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentityProfile {
    /// Chrome-shaped with the Lumen token.
    ChromeLumen,
    /// Chrome-shaped without the token.
    ChromeUnbranded,
    /// Chrome branded as Google Chrome.
    ChromeBrandGoogle,
    /// Firefox on Windows.
    FirefoxWin,
}

/// The closed action vocabulary, in [`Action::rank`] order (order of preference).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    /// Extra author-level CSS for the site.
    Css {
        /// The stylesheet text.
        css: String,
    },
    /// Hide a banner, only where `text` occurs in it.
    HideBanner {
        /// CSS selector of the banner.
        selector: String,
        /// Text the element must contain for the rule to apply.
        text: String,
    },
    /// Switch on a named Chrome-ism of the engine.
    EngineFlag {
        /// Flag name known to the engine.
        flag: String,
    },
    /// A protection exception or shim.
    Protection {
        /// Built-in exception or shim name.
        name: String,
    },
    /// Present another identity profile.
    Identity {
        /// Target profile.
        profile: IdentityProfile,
    },
    /// A built-in JS transform.
    JsTransform {
        /// Id of a transform compiled into the browser.
        id: String,
    },
    /// A recorded known issue shown to the user.
    KnownIssue {
        /// Short description.
        message: String,
    },
    /// A hint for the «open in another browser» hand-off.
    HandoffHint {
        /// Why the page needs another browser.
        reason: String,
    },
}

impl Action {
    /// Position in the order of preference (ADR-034 §4); lower is tried first.
    #[must_use]
    pub fn rank(&self) -> u8 {
        match self {
            Self::Css { .. } => 0,
            Self::HideBanner { .. } => 1,
            Self::EngineFlag { .. } => 2,
            Self::Protection { .. } => 3,
            Self::Identity { .. } => 4,
            Self::JsTransform { .. } => 5,
            Self::KnownIssue { .. } => 6,
            Self::HandoffHint { .. } => 7,
        }
    }

    /// Allowed on a bank/payment/gov host.
    fn allowed_on_sensitive(&self) -> bool {
        matches!(self, Self::Identity { .. } | Self::Css { .. } | Self::KnownIssue { .. })
    }

    /// The free-text payloads, for the emptiness check.
    fn payloads(&self) -> Vec<&str> {
        match self {
            Self::Css { css } => vec![css],
            Self::HideBanner { selector, text } => vec![selector, text],
            Self::EngineFlag { flag } => vec![flag],
            Self::Protection { name } => vec![name],
            Self::Identity { .. } => Vec::new(),
            Self::JsTransform { id } => vec![id],
            Self::KnownIssue { message } => vec![message],
            Self::HandoffHint { reason } => vec![reason],
        }
    }
}

/// One record «root cause × site».
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompatEntry {
    /// Unique id within the pack.
    pub id: String,
    /// Root-cause class, lowercase kebab (`engine-css`, `gate-loop`, …); the KB owns the taxonomy.
    pub root_cause: String,
    /// Hosts (registrable domain or deeper) the record applies to; each also covers its subdomains.
    pub matches: Vec<String>,
    /// Site kind; see [`SiteClass`].
    #[serde(default)]
    pub site_class: SiteClass,
    /// What the record does.
    pub action: Action,
    /// `BUG-NNN` or `KB-NNN` the record is justified by.
    pub bug: String,
    /// `YYYY-MM-DD`: when the record must be reviewed (a stale one is reported by [`CompatPack::overdue`]).
    pub review_by: String,
    /// Paired tests (paths or ids); empty only with `test_exemption`.
    #[serde(default)]
    pub tests: Vec<String>,
    /// Reason there is no paired test.
    #[serde(default)]
    pub test_exemption: Option<String>,
    /// Last engine version `x.y.z` the bridge is needed for; required for `engine-*` root causes.
    #[serde(default)]
    pub engine_max: Option<String>,
}

/// A compat pack: the unit that is signed, shipped and replaced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompatPack {
    /// Wire format, [`PACK_FORMAT`].
    pub format: u32,
    /// Monotonic revision; a fetched pack must be newer than the one in use.
    pub revision: u32,
    /// The records.
    pub entries: Vec<CompatEntry>,
    /// Which trusted key signed the pack; empty in the embedded base copy.
    #[serde(default)]
    pub key_id: String,
    /// Base64 ed25519 signature; empty in the embedded base copy.
    #[serde(default)]
    pub signature: String,
}

/// Why a pack was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackError {
    /// Larger than [`MAX_PACK_BYTES`].
    TooLarge,
    /// Not JSON of this shape — includes an unknown field or an unknown action.
    Parse(String),
    /// `format` is not [`PACK_FORMAT`].
    UnsupportedFormat(u32),
    /// Signature rejected.
    Signature(ManifestVerifyError),
    /// A record breaks a rule; `entry` is its id (or its index when the id is bad).
    Invalid {
        /// The offending record.
        entry: String,
        /// Which rule.
        reason: String,
    },
}

impl std::fmt::Display for PackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLarge => write!(f, "compat pack larger than {MAX_PACK_BYTES} bytes"),
            Self::Parse(e) => write!(f, "compat pack does not parse: {e}"),
            Self::UnsupportedFormat(v) => write!(f, "compat pack format {v} is not {PACK_FORMAT}"),
            Self::Signature(e) => write!(f, "compat pack signature rejected: {e:?}"),
            Self::Invalid { entry, reason } => write!(f, "compat pack entry {entry}: {reason}"),
        }
    }
}

impl std::error::Error for PackError {}

fn invalid(entry: &CompatEntry, index: usize, reason: impl Into<String>) -> PackError {
    let name = if entry.id.is_empty() { format!("#{index}") } else { entry.id.clone() };
    PackError::Invalid { entry: name, reason: reason.into() }
}

impl CompatPack {
    /// The base copy compiled into the binary. Validated like any pack but not signed — the
    /// build is its trust anchor. A broken copy is a build defect (the unit test catches it),
    /// so at runtime it degrades to an empty pack rather than panicking.
    #[must_use]
    pub fn base(psl: &dyn PublicSuffixList) -> Self {
        let json = include_str!("../assets/compat_pack.json");
        Self::parse_unsigned(json.as_bytes(), psl).unwrap_or_else(|_| Self {
            format: PACK_FORMAT,
            revision: 0,
            entries: Vec::new(),
            key_id: String::new(),
            signature: String::new(),
        })
    }

    /// Parse and validate a pack without checking a signature (embedded base copy, tooling).
    pub fn parse_unsigned(bytes: &[u8], psl: &dyn PublicSuffixList) -> Result<Self, PackError> {
        if bytes.len() > MAX_PACK_BYTES {
            return Err(PackError::TooLarge);
        }
        let pack: Self = serde_json::from_slice(bytes).map_err(|e| PackError::Parse(e.to_string()))?;
        pack.validate(psl)?;
        Ok(pack)
    }

    /// Parse a fetched pack: the signature is checked against `trusted_keys` *before* the
    /// content is validated, so unauthenticated bytes get no further than the parser.
    pub fn parse_signed(
        bytes: &[u8],
        trusted_keys: &[(&str, [u8; 32])],
        psl: &dyn PublicSuffixList,
    ) -> Result<Self, PackError> {
        if bytes.len() > MAX_PACK_BYTES {
            return Err(PackError::TooLarge);
        }
        let pack: Self = serde_json::from_slice(bytes).map_err(|e| PackError::Parse(e.to_string()))?;
        pack.verify_with_keys(trusted_keys)?;
        pack.validate(psl)?;
        Ok(pack)
    }

    /// [`Self::parse_signed`] against [`PACK_TRUSTED_KEYS`].
    pub fn parse_signed_production(bytes: &[u8], psl: &dyn PublicSuffixList) -> Result<Self, PackError> {
        Self::parse_signed(bytes, PACK_TRUSTED_KEYS, psl)
    }

    /// The exact bytes [`Self::signature`] covers: every field but `signature`, through a
    /// dedicated struct so a field added to the wire type is signed only by a deliberate change.
    fn signing_body(&self) -> Vec<u8> {
        #[derive(Serialize)]
        struct SignedFields<'a> {
            format: u32,
            revision: u32,
            entries: &'a [CompatEntry],
            key_id: &'a str,
        }
        serde_json::to_vec(&SignedFields {
            format: self.format,
            revision: self.revision,
            entries: &self.entries,
            key_id: &self.key_id,
        })
        .unwrap_or_default()
    }

    /// Sign the pack: sets `key_id`, then `signature` over the body under the pack domain tag.
    pub fn sign(&mut self, key_id: &str, signing_key: &ed25519_dalek::SigningKey) {
        self.key_id = key_id.to_string();
        self.signature = sign_tagged(PACK_TAG, &self.signing_body(), signing_key);
    }

    /// Check the signature against `trusted_keys` (does not validate the records).
    pub fn verify_with_keys(&self, trusted_keys: &[(&str, [u8; 32])]) -> Result<(), PackError> {
        let key = trusted_keys
            .iter()
            .find(|(id, _)| *id == self.key_id)
            .map(|(_, k)| *k)
            .ok_or(PackError::Signature(ManifestVerifyError::UnknownKeyId))?;
        verify_tagged(PACK_TAG, &self.signing_body(), &self.signature, &key).map_err(PackError::Signature)
    }

    /// Check every rule of ADR-034 §4 on the records.
    pub fn validate(&self, psl: &dyn PublicSuffixList) -> Result<(), PackError> {
        if self.format != PACK_FORMAT {
            return Err(PackError::UnsupportedFormat(self.format));
        }
        if self.entries.len() > MAX_ENTRIES {
            return Err(PackError::Invalid { entry: "pack".into(), reason: format!("more than {MAX_ENTRIES} entries") });
        }
        let mut ids = HashSet::new();
        let mut transforms = 0;
        for (i, e) in self.entries.iter().enumerate() {
            validate_entry(e, i, psl)?;
            if !ids.insert(e.id.as_str()) {
                return Err(invalid(e, i, "duplicate id"));
            }
            if matches!(e.action, Action::JsTransform { .. }) {
                transforms += 1;
            }
        }
        if transforms > MAX_JS_TRANSFORMS {
            return Err(PackError::Invalid {
                entry: "pack".into(),
                reason: format!("{transforms} js_transform records, at most {MAX_JS_TRANSFORMS}"),
            });
        }
        Ok(())
    }

    /// Records that apply to `host` (lowercase ASCII, as `Url::host_ascii` gives), best action
    /// first; equal ranks keep pack order.
    #[must_use]
    pub fn entries_for_host(&self, host: &str) -> Vec<&CompatEntry> {
        let host = host.trim_end_matches('.');
        let mut hits: Vec<&CompatEntry> = self.entries.iter().filter(|e| e.matches.iter().any(|p| host_covered(host, p))).collect();
        hits.sort_by_key(|e| e.action.rank());
        hits
    }

    /// Records for a navigation target; only `http`/`https` URLs with a host match.
    #[must_use]
    pub fn entries_for_url(&self, url: &lumen_core::url::Url) -> Vec<&CompatEntry> {
        if !matches!(url.scheme(), "http" | "https") {
            return Vec::new();
        }
        match url.host_ascii() {
            Ok(host) => self.entries_for_host(&host),
            Err(_) => Vec::new(),
        }
    }

    /// Records whose `review_by` is before `today` (`YYYY-MM-DD`) — for the health metric and tooling.
    #[must_use]
    pub fn overdue(&self, today: &str) -> Vec<&CompatEntry> {
        self.entries.iter().filter(|e| e.review_by.as_str() < today).collect()
    }
}

/// `host` equals `pattern` or is a subdomain of it.
fn host_covered(host: &str, pattern: &str) -> bool {
    host == pattern || host.strip_suffix(pattern).is_some_and(|rest| rest.ends_with('.'))
}

fn validate_entry(e: &CompatEntry, index: usize, psl: &dyn PublicSuffixList) -> Result<(), PackError> {
    let bad = |reason: &str| Err(invalid(e, index, reason));
    if e.id.is_empty() || e.id.len() > 64 || !e.id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_') {
        return bad("id must be 1..64 chars of [a-z0-9_-]");
    }
    if e.root_cause.is_empty() || e.root_cause.len() > 48 || !e.root_cause.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-') {
        return bad("root_cause must be lowercase kebab");
    }
    if e.matches.is_empty() || e.matches.len() > MAX_PATTERNS {
        return bad("matches must hold 1..16 hosts");
    }
    for p in &e.matches {
        if let Err(why) = check_pattern(p, psl) {
            return Err(invalid(e, index, format!("match {p:?}: {why}")));
        }
    }
    if !is_reference(&e.bug) {
        return bad("bug must be BUG-<n> or KB-<n>");
    }
    if !is_date(&e.review_by) {
        return bad("review_by must be a valid YYYY-MM-DD date");
    }
    let exemption = e.test_exemption.as_deref().is_some_and(|r| !r.trim().is_empty());
    if e.tests.iter().any(|t| t.trim().is_empty()) || (e.tests.is_empty() && !exemption) {
        return bad("needs paired tests or a non-empty test_exemption");
    }
    if e.action.payloads().iter().any(|s| s.trim().is_empty()) {
        return bad("action payload is empty");
    }
    if e.root_cause.starts_with("engine-") {
        match e.engine_max.as_deref().map(Version::parse) {
            Some(Some(_)) => {}
            _ => return bad("engine-* needs engine_max as x.y.z"),
        }
    } else if e.engine_max.is_some() {
        return bad("engine_max only belongs to engine-* root causes");
    }
    let sensitive = e.site_class != SiteClass::General || e.matches.iter().any(|p| SENSITIVE_DOMAINS.iter().any(|d| host_covered(p, d)));
    if sensitive && !e.action.allowed_on_sensitive() {
        return bad("bank/payment/gov hosts accept only identity, css and known_issue");
    }
    Ok(())
}

/// A pattern is a plain host no broader than eTLD+1.
fn check_pattern(p: &str, psl: &dyn PublicSuffixList) -> Result<(), &'static str> {
    if p.is_empty() || p.len() > 253 {
        return Err("empty or too long");
    }
    if !p.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.') {
        return Err("only lowercase ASCII letters, digits, '-' and '.' (no wildcards, punycode for IDN)");
    }
    if p.starts_with('.') || p.ends_with('.') || p.contains("..") {
        return Err("empty label");
    }
    if p.split('.').any(|l| l.len() > 63 || l.starts_with('-') || l.ends_with('-')) {
        return Err("bad label");
    }
    if p.split('.').all(|l| l.bytes().all(|b| b.is_ascii_digit())) {
        return Err("IP literal");
    }
    if psl.registrable_domain(p).is_none() {
        return Err("broader than a registrable domain (eTLD+1)");
    }
    Ok(())
}

fn is_reference(s: &str) -> bool {
    ["BUG-", "KB-"]
        .iter()
        .any(|pre| s.strip_prefix(pre).is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())))
}

fn is_date(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return false;
    }
    let digits = |r: std::ops::Range<usize>| -> Option<u32> {
        let part = s.get(r)?;
        if part.bytes().all(|c| c.is_ascii_digit()) { part.parse().ok() } else { None }
    };
    let (Some(y), Some(m), Some(d)) = (digits(0..4), digits(5..7), digits(8..10)) else {
        return false;
    };
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let days = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    y >= 2000 && (1..=days).contains(&d)
}

#[cfg(test)]
mod tests;
