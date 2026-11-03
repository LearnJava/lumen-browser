//! Browser identity — the single source for every layer that names the browser
//! (BUG-1614, ADR-034 §1).
//!
//! The HTTP `User-Agent`, `Sec-CH-UA*`, `navigator.userAgent` / `appVersion` /
//! `vendor` / `productSub` and `navigator.userAgentData` (window, workers,
//! iframes) are all derived from one User-Agent string here, so they cannot
//! contradict each other. The Chromium major comes from [`chrome_major!`].

use std::sync::atomic::{AtomicBool, Ordering};

/// Major version of the stable Chrome the Chromium profiles impersonate — the
/// single source for every Chromium UA string and for the Client Hints brands.
///
/// Must track current stable Chrome: a UA many majors behind is itself an
/// anti-bot signal (BUG-1113). A macro so `concat!` can embed it in `const`s.
#[macro_export]
macro_rules! chrome_major {
    () => {
        "153"
    };
}

/// [`chrome_major!`] as a value.
pub const CHROME_MAJOR: &str = chrome_major!();

/// Chrome User-Agent on Windows — what the `Chrome`/`Strict` HTTP profiles send
/// and what the JS layer reports by default.
pub const CHROME_USER_AGENT: &str = concat!(
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/",
    chrome_major!(),
    ".0.0.0 Safari/537.36"
);

/// Chrome User-Agent with the trailing `Lumen/x.y` token (Edge / Ladybird model,
/// ADR-034 §1) — the `chrome_lumen` identity. The version is the browser's own
/// `CARGO_PKG_VERSION`, never a literal.
pub const CHROME_LUMEN_USER_AGENT: &str = concat!(
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/",
    chrome_major!(),
    ".0.0.0 Safari/537.36 Lumen/",
    env!("CARGO_PKG_VERSION")
);

/// Firefox User-Agent on Windows — the `firefox_win` compatibility identity.
pub const FIREFOX_WIN_USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:130.0) Gecko/20100101 Firefox/130.0";

/// Edge User-Agent — the Chromium UA of the same major plus the `Edg/` token.
pub const EDGE_USER_AGENT: &str = concat!(
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/",
    chrome_major!(),
    ".0.0.0 Safari/537.36 Edg/",
    chrome_major!(),
    ".0.0.0"
);

/// Which Chromium-grammar identity the `Chrome` / `Strict` HTTP profiles wear
/// (ADR-034 §1, `identity` knob of `fingerprint.toml` / `LUMEN_IDENTITY`).
///
/// Only the User-Agent token differs: every other header, the TLS/H2 preset and
/// the rest of the fingerprint stay byte-identical, so an A/B run measures the
/// token and nothing else.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IdentityVariant {
    /// Plain Chrome grammar, no Lumen token — the default.
    #[default]
    ChromeUnbranded,
    /// Chrome grammar plus a trailing `Lumen/x.y` and a `Lumen` Client Hints brand.
    ChromeLumen,
}

impl IdentityVariant {
    /// Parse a knob value (`chrome_unbranded` | `chrome_lumen`, case-insensitive,
    /// `-` accepted for `_`).
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().replace('-', "_").as_str() {
            "chrome_unbranded" => Some(Self::ChromeUnbranded),
            "chrome_lumen" => Some(Self::ChromeLumen),
            _ => None,
        }
    }

    /// The Chromium User-Agent this variant sends.
    #[must_use]
    pub const fn user_agent(self) -> &'static str {
        match self {
            Self::ChromeUnbranded => CHROME_USER_AGENT,
            Self::ChromeLumen => CHROME_LUMEN_USER_AGENT,
        }
    }
}

/// Complete per-site identity selected by a compat-pack `identity` action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentityProfile {
    /// Chrome grammar plus the Lumen token and brand.
    ChromeLumen,
    /// Chrome grammar with Chromium Client Hints brands.
    ChromeUnbranded,
    /// The same Chrome UA with Google Chrome Client Hints brands.
    ChromeBrandGoogle,
    /// Firefox on Windows, without Chromium Client Hints.
    FirefoxWin,
}

impl IdentityProfile {
    /// The process-wide A/B variant as a complete profile.
    #[must_use]
    pub const fn from_variant(variant: IdentityVariant) -> Self {
        match variant {
            IdentityVariant::ChromeUnbranded => Self::ChromeUnbranded,
            IdentityVariant::ChromeLumen => Self::ChromeLumen,
        }
    }

    /// Build every JS- and HTTP-visible value from this one profile.
    #[must_use]
    pub fn identity(self) -> Identity {
        let ua = match self {
            Self::ChromeLumen => CHROME_LUMEN_USER_AGENT,
            Self::ChromeUnbranded | Self::ChromeBrandGoogle => CHROME_USER_AGENT,
            Self::FirefoxWin => FIREFOX_WIN_USER_AGENT,
        };
        let mut identity = Identity::from_user_agent(ua);
        if self == Self::ChromeBrandGoogle
            && let Some(data) = identity.ua_data.as_mut()
        {
            for (brand, _) in data.brands.iter_mut().chain(data.full_version_list.iter_mut()) {
                if brand == "Chromium" {
                    *brand = "Google Chrome".to_owned();
                }
            }
        }
        identity
    }
}

/// Process-global active variant: `false` = unbranded. Set once at startup by the
/// shell (`config::init_global`) before any request is built, read by the HTTP
/// header builders and the JS identity so they cannot disagree.
static LUMEN_TOKEN: AtomicBool = AtomicBool::new(false);

/// Select the process-global identity variant.
pub fn set_identity_variant(variant: IdentityVariant) {
    LUMEN_TOKEN.store(variant == IdentityVariant::ChromeLumen, Ordering::Relaxed);
}

/// The process-global identity variant ([`IdentityVariant::ChromeUnbranded`] until set).
#[must_use]
pub fn identity_variant() -> IdentityVariant {
    if LUMEN_TOKEN.load(Ordering::Relaxed) {
        IdentityVariant::ChromeLumen
    } else {
        IdentityVariant::ChromeUnbranded
    }
}

/// The Chromium User-Agent the `Chrome` / `Strict` profiles send right now.
#[must_use]
pub fn chrome_user_agent() -> &'static str {
    identity_variant().user_agent()
}

/// UA family, detected from the User-Agent string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UaFamily {
    /// Chrome / Chromium (`Chrome/NNN`).
    Chrome,
    /// Chromium with the `Edg/` token.
    Edge,
    /// Gecko (`Firefox/NNN`), incl. Tor Browser.
    Firefox,
    /// WebKit with `Version/` + `Safari/`.
    Safari,
    /// Anything else (Lumen's own `Lumen/x.y`).
    Other,
}

/// `navigator.userAgentData` payload (UA-CH). Only Chromium-family identities have it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UaData {
    /// Low-entropy brand list: `(brand, major)`.
    pub brands: Vec<(String, String)>,
    /// High-entropy brand list: `(brand, full version)`.
    pub full_version_list: Vec<(String, String)>,
    /// `Sec-CH-UA-Platform` / `userAgentData.platform`.
    pub platform: String,
    /// `platformVersion` high-entropy hint.
    pub platform_version: String,
    /// `uaFullVersion` high-entropy hint.
    pub full_version: String,
}

/// Everything a page can read about the browser's name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    /// Detected family.
    pub family: UaFamily,
    /// `User-Agent` / `navigator.userAgent`.
    pub user_agent: String,
    /// `navigator.appVersion` — the UA without the leading `Mozilla/`.
    pub app_version: String,
    /// `navigator.vendor`.
    pub vendor: String,
    /// `navigator.productSub`.
    pub product_sub: String,
    /// `navigator.platform` implied by the UA.
    pub platform: String,
    /// `navigator.userAgentData` (Chromium only).
    pub ua_data: Option<UaData>,
}

const GREASE_CHARS: [&str; 11] = [" ", "(", ":", "-", ".", "/", ")", ";", "=", "?", "_"];
const GREASE_VERSIONS: [&str; 3] = ["8", "99", "24"];

/// Chromium's GREASE brand list (`GenerateBrandVersionList`): the real brands
/// plus one `Not?A_Brand` entry whose spelling, version and position are a
/// function of the major. `grease_suffix` is appended to the GREASE version
/// in the full-version list.
fn grease_list(major: u32, real: &[(&str, String)], grease_suffix: &str) -> Vec<(String, String)> {
    let seed = major as usize;
    let grease_brand = format!(
        "Not{}A{}Brand",
        GREASE_CHARS[seed % GREASE_CHARS.len()],
        GREASE_CHARS[(seed + 1) % GREASE_CHARS.len()]
    );
    let grease_version = format!("{}{grease_suffix}", GREASE_VERSIONS[seed % GREASE_VERSIONS.len()]);
    let mut out: Vec<(String, String)> =
        real.iter().map(|(b, v)| ((*b).to_string(), v.clone())).collect();
    out.insert(seed % (real.len() + 1), (grease_brand, grease_version));
    out
}

/// `Sec-CH-UA` structured-header value for a brand list:
/// `"Not_A Brand";v="8", "Chromium";v="153"`.
#[must_use]
pub fn sec_ch_ua_value(brands: &[(String, String)]) -> String {
    brands
        .iter()
        .map(|(b, v)| format!("\"{b}\";v=\"{v}\""))
        .collect::<Vec<_>>()
        .join(", ")
}

fn parse_major(ua: &str, token: &str) -> Option<u32> {
    let rest = ua.split(token).nth(1)?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// Dotted version after `Lumen/` (`0.5.0`), if the UA carries the Lumen token.
fn lumen_token_version(ua: &str) -> Option<String> {
    let rest = ua.split("Lumen/").nth(1)?;
    let v: String = rest.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
    (!v.is_empty()).then_some(v)
}

/// `(navigator.platform, UA-CH platform)` implied by the UA string.
fn detect_platform(ua: &str) -> (&'static str, &'static str) {
    if ua.contains("Macintosh") {
        ("MacIntel", "macOS")
    } else if ua.contains("Linux") || ua.contains("X11") {
        ("Linux x86_64", "Linux")
    } else {
        ("Win32", "Windows")
    }
}

impl Identity {
    /// Derive the whole identity from a User-Agent string.
    #[must_use]
    pub fn from_user_agent(ua: &str) -> Self {
        let (family, major) = if let Some(m) = parse_major(ua, "Edg/") {
            (UaFamily::Edge, Some(m))
        } else if let Some(m) = parse_major(ua, "Chrome/") {
            (UaFamily::Chrome, Some(m))
        } else if ua.contains("Firefox/") {
            (UaFamily::Firefox, None)
        } else if ua.contains("Safari/") && ua.contains("Version/") {
            (UaFamily::Safari, None)
        } else {
            (UaFamily::Other, None)
        };
        let (nav_platform, ch_platform) = detect_platform(ua);
        let (vendor, product_sub) = match family {
            UaFamily::Chrome | UaFamily::Edge => ("Google Inc.", "20030107"),
            UaFamily::Safari => ("Apple Computer, Inc.", "20030107"),
            UaFamily::Firefox => ("", "20100101"),
            UaFamily::Other => ("", "20030107"),
        };
        let app_version = ua.strip_prefix("Mozilla/").unwrap_or(ua).to_string();
        let ua_data = major.map(|m| {
            let mut real = vec![("Chromium", m.to_string())];
            let mut real_full = vec![("Chromium", format!("{m}.0.0.0"))];
            if family == UaFamily::Edge {
                real.push(("Microsoft Edge", m.to_string()));
                real_full.push(("Microsoft Edge", format!("{m}.0.0.0")));
            }
            // Brands always follow the UA (ADR-034): the token brings its brand.
            if let Some(v) = lumen_token_version(ua) {
                real.push(("Lumen", v.split('.').next().unwrap_or(&v).to_string()));
                real_full.push(("Lumen", v));
            }
            UaData {
                brands: grease_list(m, &real, ""),
                full_version_list: grease_list(m, &real_full, ".0.0.0"),
                platform: ch_platform.to_string(),
                platform_version: if ch_platform == "Windows" { "10.0.0" } else { "0.0.0" }.to_string(),
                full_version: format!("{m}.0.0.0"),
            }
        });
        Self {
            family,
            user_agent: ua.to_string(),
            app_version,
            vendor: vendor.to_string(),
            product_sub: product_sub.to_string(),
            platform: nav_platform.to_string(),
            ua_data,
        }
    }

    /// Low-entropy Client Hints Chromium sends on every request
    /// (`Sec-CH-UA`, `Sec-CH-UA-Mobile`, `Sec-CH-UA-Platform`); empty for
    /// families without `userAgentData`.
    #[must_use]
    pub fn low_entropy_client_hints(&self) -> Vec<(&'static str, String)> {
        match &self.ua_data {
            Some(d) => vec![
                ("Sec-CH-UA", sec_ch_ua_value(&d.brands)),
                ("Sec-CH-UA-Mobile", "?0".to_string()),
                ("Sec-CH-UA-Platform", format!("\"{}\"", d.platform)),
            ],
            None => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHROME_UA: &str = CHROME_USER_AGENT;

    #[test]
    fn chrome_identity_is_consistent() {
        let id = Identity::from_user_agent(CHROME_UA);
        assert_eq!(id.family, UaFamily::Chrome);
        assert!(id.app_version.starts_with("5.0 (Windows NT 10.0"));
        assert!(id.app_version.ends_with("Safari/537.36"));
        assert!(id.app_version.contains(&format!("Chrome/{CHROME_MAJOR}")));
        assert_eq!(id.platform, "Win32");
        let d = id.ua_data.unwrap();
        assert!(d.brands.contains(&("Chromium".into(), CHROME_MAJOR.into())));
        assert_eq!(d.full_version, format!("{CHROME_MAJOR}.0.0.0"));
        assert_eq!(d.brands.len(), 2);
    }

    #[test]
    fn grease_brand_follows_the_major() {
        // seed 153: 153%11=10 -> "_", 154%11=0 -> " "; 153%3=0 -> "8"
        let d = Identity::from_user_agent(CHROME_UA).ua_data.unwrap();
        assert!(d.brands.iter().any(|(b, v)| b == "Not_A Brand" && v == "8"));
        assert!(d.full_version_list.iter().any(|(b, v)| b == "Not_A Brand" && v == "8.0.0.0"));
    }

    #[test]
    fn sec_ch_ua_is_structured_header_list() {
        let h = Identity::from_user_agent(CHROME_UA).low_entropy_client_hints();
        assert_eq!(h.len(), 3);
        assert!(h[0].1.contains(&format!("\"Chromium\";v=\"{CHROME_MAJOR}\"")));
        assert!(h[0].1.contains(", "));
        assert_eq!(h[1].1, "?0");
        assert_eq!(h[2].1, "\"Windows\"");
    }

    #[test]
    fn edge_adds_its_brand() {
        let ua = format!("{CHROME_UA} Edg/{CHROME_MAJOR}.0.0.0");
        let d = Identity::from_user_agent(&ua).ua_data.unwrap();
        assert!(d.brands.iter().any(|(b, _)| b == "Microsoft Edge"));
        assert_eq!(d.brands.len(), 3);
    }

    #[test]
    fn firefox_tor_has_no_ua_data_and_gecko_fields() {
        let id = Identity::from_user_agent(
            "Mozilla/5.0 (Windows NT 10.0; rv:128.0) Gecko/20100101 Firefox/128.0",
        );
        assert_eq!(id.family, UaFamily::Firefox);
        assert!(id.ua_data.is_none());
        assert_eq!(id.vendor, "");
        assert_eq!(id.product_sub, "20100101");
        assert!(id.low_entropy_client_hints().is_empty());
    }

    #[test]
    fn safari_and_linux_platforms() {
        let s = Identity::from_user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15");
        assert_eq!((s.family, s.platform.as_str(), s.vendor.as_str()), (UaFamily::Safari, "MacIntel", "Apple Computer, Inc."));
        let f = Identity::from_user_agent("Mozilla/5.0 (X11; Linux x86_64; rv:130.0) Gecko/20100101 Firefox/130.0");
        assert_eq!(f.platform, "Linux x86_64");
    }

    #[test]
    fn variant_parses_both_spellings_and_rejects_the_rest() {
        assert_eq!(IdentityVariant::parse("chrome_lumen"), Some(IdentityVariant::ChromeLumen));
        assert_eq!(IdentityVariant::parse(" Chrome-Unbranded "), Some(IdentityVariant::ChromeUnbranded));
        assert_eq!(IdentityVariant::parse("firefox_win"), None);
        assert_eq!(IdentityVariant::default(), IdentityVariant::ChromeUnbranded);
    }

    #[test]
    fn lumen_variant_is_chrome_plus_trailing_token_only() {
        let version = env!("CARGO_PKG_VERSION");
        assert_eq!(CHROME_LUMEN_USER_AGENT, format!("{CHROME_USER_AGENT} Lumen/{version}"));
        assert_eq!(IdentityVariant::ChromeLumen.user_agent(), CHROME_LUMEN_USER_AGENT);
        assert_eq!(IdentityVariant::ChromeUnbranded.user_agent(), CHROME_USER_AGENT);
    }

    #[test]
    fn lumen_token_adds_its_brand_to_client_hints_and_nothing_else() {
        let plain = Identity::from_user_agent(CHROME_USER_AGENT);
        let lumen = Identity::from_user_agent(CHROME_LUMEN_USER_AGENT);
        let major = env!("CARGO_PKG_VERSION").split('.').next().unwrap();
        let d = lumen.ua_data.as_ref().unwrap();
        assert!(d.brands.contains(&("Chromium".into(), CHROME_MAJOR.into())));
        assert!(d.brands.contains(&("Lumen".into(), major.into())));
        assert_eq!(d.brands.len(), 3);
        assert!(d.full_version_list.contains(&("Lumen".into(), env!("CARGO_PKG_VERSION").into())));
        assert_eq!(lumen.family, UaFamily::Chrome);
        assert!(lumen.user_agent.ends_with(&format!("Lumen/{}", env!("CARGO_PKG_VERSION"))));
        assert!(lumen.app_version.ends_with(&format!("Lumen/{}", env!("CARGO_PKG_VERSION"))));
        // Platform, vendor and the remaining hints are those of the plain identity.
        assert_eq!(
            (&lumen.platform, &lumen.vendor, &lumen.product_sub),
            (&plain.platform, &plain.vendor, &plain.product_sub)
        );
        let (pd, ld) = (plain.ua_data.as_ref().unwrap(), d);
        assert_eq!((&pd.platform, &pd.platform_version, &pd.full_version), (&ld.platform, &ld.platform_version, &ld.full_version));
        assert!(lumen.low_entropy_client_hints()[0].1.contains("\"Lumen\";v=\""));
        assert!(!plain.low_entropy_client_hints()[0].1.contains("Lumen"));
    }

    #[test]
    fn lumen_token_ua_is_other() {
        let id = Identity::from_user_agent("Lumen/0.5.0");
        assert_eq!(id.family, UaFamily::Other);
        assert_eq!(id.app_version, "Lumen/0.5.0");
        assert!(id.ua_data.is_none());
    }
}
