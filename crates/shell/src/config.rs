//! Fingerprint profile configuration (9F.1).
//!
//! Lets the user pin the browser's anti-fingerprinting surface to a fixed set
//! of values via a small TOML file, so every page sees a consistent, chosen
//! identity instead of the engine defaults.
//!
//! Path (resolved by [`config_path`]):
//! - Windows: `%APPDATA%\lumen\fingerprint.toml`
//! - other:   `$XDG_CONFIG_HOME/lumen/fingerprint.toml`, else `~/.config/lumen/fingerprint.toml`
//!
//! The file is a flat `key = value` subset of TOML (no tables/arrays). Unknown
//! keys are ignored; malformed values fall back to the profile default. Example:
//!
//! ```toml
//! # ~/.config/lumen/fingerprint.toml
//! http_profile        = "chrome"        # chrome|firefox|safari|edge|tor|lumen|strict
//! tls_profile         = "standard"      # standard|strict|tor (default: derived from http_profile)
//! screen_width        = 1920
//! screen_height       = 1080
//! color_depth         = 24
//! timezone_offset     = 0               # minutes, getTimezoneOffset() convention (+ = behind UTC)
//! hardware_concurrency = 8
//! device_memory       = 8
//! platform            = "Win32"
//! languages           = "en-US,en"      # comma-separated; first entry = navigator.language
//! doh_url             = "https://cloudflare-dns.com/dns-query"  # DNS over HTTPS resolver (optional)
//! ```
//!
//! Applied at startup: [`FingerprintProfile::install_navigator`] pushes the
//! navigator/screen/timezone values into the process-global JS profile, and
//! [`FingerprintProfile::apply_http`] stamps the HTTP/TLS fingerprint onto an
//! [`HttpClient`].

use lumen_core::url::Url;
use lumen_network::{HttpClient, HttpProfile, Socks5Proxy, TlsProfile};
use std::path::PathBuf;
use std::sync::OnceLock;

/// Process-global fingerprint profile, loaded once at startup.
///
/// Set via [`init_global`]; read via [`global`]. Falls back to
/// [`FingerprintProfile::default`] when never initialised (e.g. in tests).
static GLOBAL: OnceLock<FingerprintProfile> = OnceLock::new();

/// Install the process-global fingerprint profile. Idempotent: the first call
/// wins, subsequent calls are ignored (returns whether this call set it).
pub fn init_global(profile: FingerprintProfile) -> bool {
    GLOBAL.set(profile).is_ok()
}

/// Return the process-global fingerprint profile, or the default if unset.
#[must_use]
pub fn global() -> &'static FingerprintProfile {
    GLOBAL.get_or_init(FingerprintProfile::default)
}

/// Install the built-in ad/tracker filter as the process-global
/// (`lumen_network::install_global_adblock_filter`) and enable it.
///
/// Call once at startup. The filter (`EasyListFilter` over the bundled
/// `DefaultFilterList`) is then consulted by every `HttpClient` request on every
/// fetch path; the per-tab checkbox flips it via
/// `lumen_network::set_global_adblock_enabled`. Enabled here to match the
/// default `TabEntry::adblock = true` of the initial tab.
pub fn init_adblock() {
    use lumen_core::ext::FilterListSource as _;
    let rules = lumen_network::DefaultFilterList.fetch_rules().unwrap_or_default();
    let filter = std::sync::Arc::new(lumen_network::EasyListFilter::parse(&rules));
    lumen_network::install_global_adblock_filter(filter);
    lumen_network::set_global_adblock_enabled(true);
}

/// User-configurable fingerprint identity (9F.1).
///
/// Fields with `None` (TLS) or carrying the [`Default`] navigator values mean
/// "use the engine default". Built from `fingerprint.toml` via [`parse`], or
/// [`FingerprintProfile::default`] when no config file is present.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FingerprintProfile {
    /// HTTP layer fingerprint (header order/casing, H2 SETTINGS, Client Hints).
    /// Defaults to [`HttpProfile::Chrome`].
    pub http_profile: HttpProfile,
    /// TLS ClientHello fingerprint. `None` derives it from `http_profile` via
    /// `lumen_network::http_to_tls_profile`; `Some` overrides explicitly.
    pub tls_profile: Option<TlsProfile>,
    /// `navigator.hardwareConcurrency` — reported logical CPU count.
    pub hardware_concurrency: u32,
    /// `navigator.deviceMemory` — reported RAM in GiB.
    pub device_memory: u32,
    /// `navigator.platform` — UA platform string.
    pub platform: String,
    /// `navigator.languages` (ordered); first entry is `navigator.language`.
    pub languages: Vec<String>,
    /// `screen.width` / `screen.availWidth` in CSS pixels.
    pub screen_width: u32,
    /// `screen.height` / `screen.availHeight` in CSS pixels.
    pub screen_height: u32,
    /// `screen.colorDepth` / `screen.pixelDepth` in bits.
    pub color_depth: u32,
    /// `Date.prototype.getTimezoneOffset()` value in minutes (+ = behind UTC).
    pub timezone_offset: i32,
    /// DNS over HTTPS resolver URL. `None` disables DoH; falls back to system DNS.
    /// Example: `https://cloudflare-dns.com/dns-query`.
    pub doh_url: Option<String>,
    /// HTTP proxy URL. `None` means no proxy; goes directly to target.
    /// Example: `http://proxy.local:3128` or `http://user:pass@proxy.local:8080`.
    pub proxy: Option<String>,
    /// SOCKS5 proxy URL for tunnelling all connections (RFC 1928).
    /// Format: `socks5://[user:pass@]host:port`.
    /// `None` with `http_profile = TorBrowser` auto-wires `socks5://127.0.0.1:9050`
    /// (local Tor daemon default); set explicitly to override the address or
    /// to use SOCKS5 without the TorBrowser HTTP fingerprint.
    pub socks5_proxy: Option<String>,
    /// When `true`, no cookies, localStorage, or session data are written to
    /// disk.  All storage is in-memory and discarded when the session ends.
    /// Automatically `true` when `http_profile = TorBrowser` and not
    /// overridden explicitly.
    pub no_persistent_state: bool,
}

impl Default for FingerprintProfile {
    fn default() -> Self {
        // Defaults mirror lumen_js::NavigatorProfile::default() and the
        // HttpClient default profile, so an absent config changes nothing.
        Self {
            http_profile: HttpProfile::Chrome,
            tls_profile: None,
            hardware_concurrency: 2,
            device_memory: 8,
            platform: "Win32".to_string(),
            languages: vec!["en-US".to_string(), "en".to_string()],
            screen_width: 1920,
            screen_height: 1080,
            color_depth: 24,
            timezone_offset: 0,
            doh_url: None,
            proxy: None,
            socks5_proxy: None,
            no_persistent_state: false,
        }
    }
}

impl FingerprintProfile {
    /// Resolve the effective TLS profile: explicit override, else derived from
    /// the HTTP profile.
    #[must_use]
    pub fn effective_tls_profile(&self) -> TlsProfile {
        self.tls_profile
            .unwrap_or_else(|| lumen_network::http_to_tls_profile(self.http_profile))
    }

    /// Build the JS-side [`lumen_js::NavigatorProfile`] from this config.
    ///
    /// When `http_profile == TorBrowser`, screen is pinned to 1000×900 and
    /// `platform` to `"Win32"` to match the Tor Browser uniform fingerprint
    /// (anti-fingerprinting via population uniformity, TB §10.3).
    #[cfg(feature = "quickjs")]
    #[must_use]
    pub fn navigator_profile(&self) -> lumen_js::NavigatorProfile {
        let is_tor = self.http_profile == HttpProfile::TorBrowser;
        lumen_js::NavigatorProfile {
            hardware_concurrency: self.hardware_concurrency,
            device_memory: self.device_memory,
            // Tor Browser always reports "Win32" regardless of actual OS.
            platform: if is_tor { "Win32".to_string() } else { self.platform.clone() },
            // Tor Browser pins Accept-Language to en-US to avoid locale leakage.
            languages: if is_tor {
                vec!["en-US".to_string(), "en".to_string()]
            } else {
                self.languages.clone()
            },
            // Tor Browser letterboxes viewport; screen reports 1000×900 default.
            screen_width: if is_tor { 1000 } else { self.screen_width },
            screen_height: if is_tor { 900 } else { self.screen_height },
            color_depth: self.color_depth,
            timezone_offset: if is_tor { 0 } else { self.timezone_offset },
        }
    }

    /// Install the navigator/screen/timezone values into the process-global JS
    /// profile. Must be called once at startup, before any page loads.
    #[cfg(feature = "quickjs")]
    pub fn install_navigator(&self) {
        lumen_js::set_navigator_profile(self.navigator_profile());
    }

    /// Stamp the HTTP and TLS fingerprint onto an [`HttpClient`] builder.
    #[must_use]
    pub fn apply_http(&self, mut client: HttpClient) -> HttpClient {
        client = client
            .with_fingerprint_profile(self.http_profile)
            .with_tls_profile(self.effective_tls_profile());

        // Wire DoH resolver if configured
        if let Some(doh_url) = &self.doh_url
            && let Ok(endpoint) = Url::parse(doh_url)
        {
            let bootstrap_client = std::sync::Arc::new(
                HttpClient::new()
                    .with_fingerprint_profile(self.http_profile)
                    .with_tls_profile(self.effective_tls_profile()),
            );
            let doh_resolver = std::sync::Arc::new(
                lumen_network::DohResolver::new(endpoint, bootstrap_client),
            );
            let cached = std::sync::Arc::new(
                lumen_network::CachedDnsResolver::new(doh_resolver),
            );
            client = client.with_dns_resolver(cached);
        }

        // Wire HTTP proxy if configured
        if let Some(proxy_str) = &self.proxy
            && let Some(proxy) = parse_http_proxy(proxy_str)
        {
            client = client.with_proxy(std::sync::Arc::new(proxy));
        }

        // Wire SOCKS5 proxy.
        // Explicit `socks5_proxy` field takes precedence; otherwise, when the
        // TorBrowser HTTP profile is active, auto-wire to the local Tor daemon
        // at 127.0.0.1:9050 (the standard Tor socks5 port).
        if let Some(s5) = self.effective_socks5_proxy() {
            client = client.with_socks5_proxy(std::sync::Arc::new(s5));
        }

        client
    }

    /// Resolve the effective SOCKS5 proxy: explicit override first, then
    /// auto-detect for TorBrowser profile.
    ///
    /// Returns `None` when no SOCKS5 tunnel should be used.
    #[must_use]
    pub fn effective_socks5_proxy(&self) -> Option<Socks5Proxy> {
        if let Some(s5_str) = &self.socks5_proxy {
            return parse_socks5_proxy(s5_str);
        }
        // Auto-wire for TorBrowser: connect through local Tor daemon.
        if self.http_profile == HttpProfile::TorBrowser {
            return Some(Socks5Proxy::new("127.0.0.1", 9050));
        }
        None
    }
}

/// Resolve the platform-specific path to `fingerprint.toml`.
///
/// Returns `None` only when neither `%APPDATA%`/`XDG_CONFIG_HOME`/`HOME` (nor
/// `USERPROFILE` on Windows) is set — in which case there is nowhere to look.
#[must_use]
pub fn config_path() -> Option<PathBuf> {
    let base: PathBuf = if cfg!(windows) {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))?
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?
    };
    Some(base.join("lumen").join("fingerprint.toml"))
}

/// Load and parse the fingerprint profile from the default config path.
///
/// Returns `None` when the file does not exist or cannot be read; returns
/// `Some` (with defaults for any missing/invalid keys) when a file is present.
#[must_use]
pub fn load() -> Option<FingerprintProfile> {
    let path = config_path()?;
    let contents = std::fs::read_to_string(&path).ok()?;
    Some(parse(&contents))
}

/// Parse a flat `key = value` TOML subset into a [`FingerprintProfile`].
///
/// Comments (`#` to end of line) and blank lines are skipped. Values may be
/// optionally quoted (`"..."` or `'...'`). Unknown keys are ignored; invalid
/// values for a known key leave that field at its default.
#[must_use]
pub fn parse(contents: &str) -> FingerprintProfile {
    let mut p = FingerprintProfile::default();
    for raw_line in contents.lines() {
        let line = strip_comment(raw_line).trim();
        if line.is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = unquote(value.trim());
        apply_key(&mut p, key, value);
    }
    p
}

/// Drop a trailing `#` comment, ignoring `#` inside quoted strings.
fn strip_comment(line: &str) -> &str {
    let mut in_single = false;
    let mut in_double = false;
    for (i, ch) in line.char_indices() {
        match ch {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '#' if !in_single && !in_double => return &line[..i],
            _ => {}
        }
    }
    line
}

/// Strip a single matching pair of surrounding quotes, if present.
fn unquote(value: &str) -> &str {
    let bytes = value.as_bytes();
    if bytes.len() >= 2
        && ((bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[bytes.len() - 1] == b'\''))
    {
        &value[1..value.len() - 1]
    } else {
        value
    }
}

/// Apply a single parsed `key`/`value` pair, ignoring unknown keys and leaving
/// the field unchanged on a parse error.
fn apply_key(p: &mut FingerprintProfile, key: &str, value: &str) {
    match key {
        "http_profile" => {
            if let Some(profile) = parse_http_profile(value) {
                p.http_profile = profile;
            }
        }
        "tls_profile" => {
            if let Some(profile) = parse_tls_profile(value) {
                p.tls_profile = Some(profile);
            }
        }
        "hardware_concurrency" => {
            if let Ok(v) = value.parse() {
                p.hardware_concurrency = v;
            }
        }
        "device_memory" => {
            if let Ok(v) = value.parse() {
                p.device_memory = v;
            }
        }
        "platform" if !value.is_empty() => {
            p.platform = value.to_string();
        }
        "languages" => {
            let langs: Vec<String> = value
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect();
            if !langs.is_empty() {
                p.languages = langs;
            }
        }
        "screen_width" => {
            if let Ok(v) = value.parse() {
                p.screen_width = v;
            }
        }
        "screen_height" => {
            if let Ok(v) = value.parse() {
                p.screen_height = v;
            }
        }
        "color_depth" => {
            if let Ok(v) = value.parse() {
                p.color_depth = v;
            }
        }
        "timezone_offset" => {
            if let Ok(v) = value.parse() {
                p.timezone_offset = v;
            }
        }
        "doh_url" if !value.is_empty() => {
            p.doh_url = Some(value.to_string());
        }
        "proxy" if !value.is_empty() => {
            p.proxy = Some(value.to_string());
        }
        "socks5_proxy" | "socks5" if !value.is_empty() => {
            p.socks5_proxy = Some(value.to_string());
        }
        "no_persistent_state" => {
            p.no_persistent_state = matches!(value.to_ascii_lowercase().as_str(), "true" | "1" | "yes");
        }
        _ => {}
    }
}

/// Parse an `http_profile` value (case-insensitive) into an [`HttpProfile`].
fn parse_http_profile(value: &str) -> Option<HttpProfile> {
    match value.to_ascii_lowercase().as_str() {
        "chrome" => Some(HttpProfile::Chrome),
        "firefox" => Some(HttpProfile::Firefox),
        "safari" => Some(HttpProfile::Safari),
        "edge" => Some(HttpProfile::Edge),
        "tor" | "torbrowser" | "tor-browser" => Some(HttpProfile::TorBrowser),
        "lumen" => Some(HttpProfile::Lumen),
        "strict" => Some(HttpProfile::Strict),
        _ => None,
    }
}

/// Parse a `tls_profile` value (case-insensitive) into a [`TlsProfile`].
fn parse_tls_profile(value: &str) -> Option<TlsProfile> {
    match value.to_ascii_lowercase().as_str() {
        "standard" => Some(TlsProfile::Standard),
        "strict" => Some(TlsProfile::Strict),
        "tor" => Some(TlsProfile::Tor),
        _ => None,
    }
}

/// Parse HTTP proxy URL into an [`HttpProxy`] struct.
/// Format: `http://[user:pass@]host:port` or `https://[user:pass@]host:port` (both treated as plain HTTP).
fn parse_http_proxy(proxy_url: &str) -> Option<lumen_network::HttpProxy> {
    use lumen_network::HttpProxy;

    // Strip scheme (http:// or https://)
    let url_str = proxy_url
        .strip_prefix("http://")
        .or_else(|| proxy_url.strip_prefix("https://"))?;

    // Parse [user:pass@]host:port
    let (auth_part, host_port) = if let Some(at_idx) = url_str.rfind('@') {
        (&url_str[..at_idx], &url_str[at_idx + 1..])
    } else {
        ("", url_str)
    };

    // Parse host:port
    let (host, port_str) = host_port.rsplit_once(':')?;
    let port: u16 = port_str.parse().ok()?;

    let mut proxy = HttpProxy::new(host.to_string(), port);
    if !auth_part.is_empty()
        && let Some(colon_idx) = auth_part.find(':')
    {
        let user = &auth_part[..colon_idx];
        let pass = &auth_part[colon_idx + 1..];
        proxy = proxy.with_basic_auth(user, pass);
    }
    Some(proxy)
}

/// Parse a SOCKS5 proxy URL into a [`Socks5Proxy`] struct.
///
/// Format: `socks5://[user:pass@]host:port`
fn parse_socks5_proxy(proxy_url: &str) -> Option<Socks5Proxy> {

    let url_str = proxy_url.strip_prefix("socks5://")?;

    let (auth_part, host_port) = if let Some(at_idx) = url_str.rfind('@') {
        (&url_str[..at_idx], &url_str[at_idx + 1..])
    } else {
        ("", url_str)
    };

    let (host, port_str) = host_port.rsplit_once(':')?;
    let port: u16 = port_str.parse().ok()?;

    let mut proxy = Socks5Proxy::new(host, port);
    if !auth_part.is_empty()
        && let Some(colon_idx) = auth_part.find(':')
    {
        let user = &auth_part[..colon_idx];
        let pass = &auth_part[colon_idx + 1..];
        proxy = proxy.with_auth(user, pass);
    }
    Some(proxy)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_matches_engine_defaults() {
        let p = FingerprintProfile::default();
        assert_eq!(p.http_profile, HttpProfile::Chrome);
        assert_eq!(p.tls_profile, None);
        assert_eq!(p.effective_tls_profile(), TlsProfile::Standard);
        assert_eq!(p.hardware_concurrency, 2);
        assert_eq!(p.device_memory, 8);
        assert_eq!(p.platform, "Win32");
        assert_eq!(p.languages, vec!["en-US".to_string(), "en".to_string()]);
        assert_eq!(p.screen_width, 1920);
        assert_eq!(p.screen_height, 1080);
        assert_eq!(p.color_depth, 24);
        assert_eq!(p.timezone_offset, 0);
    }

    #[test]
    fn empty_config_is_default() {
        assert_eq!(parse(""), FingerprintProfile::default());
    }

    #[test]
    fn parses_full_config() {
        let cfg = r#"
            http_profile = "firefox"
            tls_profile = "strict"
            hardware_concurrency = 8
            device_memory = 16
            platform = "Linux x86_64"
            languages = "de-DE, de, en"
            screen_width = 2560
            screen_height = 1440
            color_depth = 30
            timezone_offset = -120
        "#;
        let p = parse(cfg);
        assert_eq!(p.http_profile, HttpProfile::Firefox);
        assert_eq!(p.tls_profile, Some(TlsProfile::Strict));
        assert_eq!(p.effective_tls_profile(), TlsProfile::Strict);
        assert_eq!(p.hardware_concurrency, 8);
        assert_eq!(p.device_memory, 16);
        assert_eq!(p.platform, "Linux x86_64");
        assert_eq!(
            p.languages,
            vec!["de-DE".to_string(), "de".to_string(), "en".to_string()]
        );
        assert_eq!(p.screen_width, 2560);
        assert_eq!(p.screen_height, 1440);
        assert_eq!(p.color_depth, 30);
        assert_eq!(p.timezone_offset, -120);
    }

    #[test]
    fn comments_and_blank_lines_ignored() {
        let cfg = "# a comment\n\n  screen_width = 1366  # inline comment\n";
        let p = parse(cfg);
        assert_eq!(p.screen_width, 1366);
        // Untouched fields keep defaults.
        assert_eq!(p.screen_height, 1080);
    }

    #[test]
    fn hash_inside_quotes_is_not_a_comment() {
        let p = parse(r#"platform = "Win#32""#);
        assert_eq!(p.platform, "Win#32");
    }

    #[test]
    fn unquoted_values_work() {
        let p = parse("http_profile = edge\nscreen_width = 800\n");
        assert_eq!(p.http_profile, HttpProfile::Edge);
        assert_eq!(p.screen_width, 800);
    }

    #[test]
    fn single_quotes_stripped() {
        let p = parse("platform = 'MacIntel'");
        assert_eq!(p.platform, "MacIntel");
    }

    #[test]
    fn unknown_keys_ignored() {
        let p = parse("totally_unknown = 42\nscreen_width = 640\n");
        assert_eq!(p.screen_width, 640);
        assert_eq!(
            p,
            FingerprintProfile {
                screen_width: 640,
                ..Default::default()
            }
        );
    }

    #[test]
    fn invalid_value_keeps_default() {
        // Non-numeric width must not clobber the default.
        let p = parse("screen_width = not_a_number");
        assert_eq!(p.screen_width, 1920);
    }

    #[test]
    fn invalid_profile_keeps_default() {
        let p = parse("http_profile = netscape");
        assert_eq!(p.http_profile, HttpProfile::Chrome);
    }

    #[test]
    fn http_profile_case_insensitive() {
        assert_eq!(parse("http_profile = CHROME").http_profile, HttpProfile::Chrome);
        assert_eq!(
            parse("http_profile = Tor-Browser").http_profile,
            HttpProfile::TorBrowser
        );
    }

    #[test]
    fn tor_http_derives_tor_tls() {
        let p = parse("http_profile = tor");
        assert_eq!(p.http_profile, HttpProfile::TorBrowser);
        assert_eq!(p.effective_tls_profile(), TlsProfile::Tor);
    }

    #[test]
    fn explicit_tls_overrides_derived() {
        let p = parse("http_profile = tor\ntls_profile = standard\n");
        assert_eq!(p.effective_tls_profile(), TlsProfile::Standard);
    }

    #[test]
    fn empty_languages_keeps_default() {
        let p = parse("languages = ,, ,");
        assert_eq!(p.languages, vec!["en-US".to_string(), "en".to_string()]);
    }

    #[test]
    fn config_path_is_some() {
        // At least one of the env vars used for resolution is set in any
        // normal environment (HOME on unix, APPDATA/USERPROFILE on Windows).
        assert!(config_path().is_some());
        let path = config_path().unwrap();
        assert!(path.ends_with("lumen/fingerprint.toml") || path.ends_with("lumen\\fingerprint.toml"));
    }

    // ── Tor / SOCKS5 tests ────────────────────────────────────────────────

    #[test]
    fn tor_profile_auto_wires_socks5() {
        let p = parse("http_profile = tor");
        let s5 = p.effective_socks5_proxy().expect("TorBrowser must auto-wire SOCKS5");
        assert_eq!(s5.host, "127.0.0.1");
        assert_eq!(s5.port, 9050);
        assert!(s5.auth.is_none());
    }

    #[test]
    fn non_tor_profile_no_auto_socks5() {
        let p = parse("http_profile = chrome");
        assert!(p.effective_socks5_proxy().is_none());
    }

    #[test]
    fn explicit_socks5_proxy_parsed() {
        let p = parse("socks5_proxy = socks5://127.0.0.1:9150");
        let s5 = p.effective_socks5_proxy().expect("explicit socks5");
        assert_eq!(s5.host, "127.0.0.1");
        assert_eq!(s5.port, 9150);
    }

    #[test]
    fn socks5_proxy_with_auth() {
        let p = parse("socks5_proxy = socks5://alice:secret@proxy.lan:1080");
        let s5 = p.effective_socks5_proxy().expect("socks5 with auth");
        assert_eq!(s5.host, "proxy.lan");
        assert_eq!(s5.port, 1080);
        let (user, pass) = s5.auth.expect("auth present");
        assert_eq!(user, "alice");
        assert_eq!(pass, "secret");
    }

    #[test]
    fn explicit_socks5_overrides_tor_auto() {
        // Explicit socks5 address overrides the auto-9050 even in Tor mode.
        let p = parse("http_profile = tor\nsocks5_proxy = socks5://10.0.0.1:9999");
        let s5 = p.effective_socks5_proxy().expect("explicit override");
        assert_eq!(s5.host, "10.0.0.1");
        assert_eq!(s5.port, 9999);
    }

    #[test]
    fn no_persistent_state_parsed() {
        let p = parse("no_persistent_state = true");
        assert!(p.no_persistent_state);
        let p2 = parse("no_persistent_state = false");
        assert!(!p2.no_persistent_state);
        let p3 = parse("no_persistent_state = 1");
        assert!(p3.no_persistent_state);
    }

    #[test]
    fn socks5_alias_key_works() {
        let p = parse("socks5 = socks5://127.0.0.1:9050");
        assert_eq!(p.socks5_proxy, Some("socks5://127.0.0.1:9050".to_string()));
    }

    #[cfg(feature = "quickjs")]
    #[test]
    fn tor_navigator_profile_pins_screen_and_platform() {
        let p = parse("http_profile = tor");
        let nav = p.navigator_profile();
        assert_eq!(nav.screen_width, 1000);
        assert_eq!(nav.screen_height, 900);
        assert_eq!(nav.platform, "Win32");
        assert_eq!(nav.languages, vec!["en-US".to_string(), "en".to_string()]);
        assert_eq!(nav.timezone_offset, 0);
    }
}
