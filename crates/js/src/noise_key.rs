//! COMPAT-PRIVACY-UNBREAK-NOISE — which seed the canvas / audio fingerprint
//! noise (ADR-007 layer 4) runs on, and for which sites it is switched off.
//!
//! The noise used to be keyed on «process start time ^ pid + full origin»: the
//! fingerprint changed on every restart (an anti-fraud system saw a new device
//! each launch), every sub-domain of one site got its own value, and nothing
//! was exempt. The key is now «profile secret + eTLD+1»:
//!
//! - a *profile secret* ([`set_noise_secret`]) is installed by the shell per
//!   browser profile and survives restarts, so a scene hashes to the same
//!   value in two runs of one profile, and to different values in two profiles;
//!   without one (Anonymous, Tor, tests) a per-run random secret is used —
//!   the old «new fingerprint every launch» behaviour, which is what those
//!   modes want;
//! - the *site* is the registrable domain, so `a.example.com` and
//!   `b.example.com` read the same noise (cross-site correlation stays
//!   impossible: two sites get two keys);
//! - *exempt* hosts (identity checks, banks, public services — [`BUILTIN_EXEMPT`]
//!   plus [`set_noise_exceptions`]) get no noise at all: their anti-fraud
//!   reads the canvas and reacts to a drifting hash.

use std::sync::{OnceLock, RwLock};

/// Hosts (and everything below them) where the noise is off by default:
/// captcha / bot-check providers, Russian state services and the large banks,
/// and the government zones. A suffix entry matches the host itself and any
/// sub-domain of it (`gov.ru` covers `www.gov.ru` and `nalog.gov.ru`).
pub(crate) const BUILTIN_EXEMPT: &[&str] = &[
    // checks
    "challenges.cloudflare.com",
    "hcaptcha.com",
    "recaptcha.net",
    "captcha.yandex.net",
    "smartcaptcha.yandexcloud.net",
    "captcha-delivery.com",
    // state services
    "gosuslugi.ru",
    "nalog.ru",
    "nalog.gov.ru",
    "mos.ru",
    "cbr.ru",
    "gov.ru",
    "gov",
    "mil",
    // banks and payments
    "sberbank.ru",
    "sber.ru",
    "sberbank.com",
    "vtb.ru",
    "alfabank.ru",
    "tbank.ru",
    "tinkoff.ru",
    "gazprombank.ru",
    "raiffeisen.ru",
    "rshb.ru",
    "psbank.ru",
    "mironline.ru",
    "nspk.ru",
    "sbp.nspk.ru",
    "rzd.ru",
];

struct State {
    /// `None` — no stable secret: use the per-run one.
    secret: Option<u64>,
    /// Lower-case extra exempt entries, on top of [`BUILTIN_EXEMPT`].
    extra: Vec<String>,
}

fn state() -> &'static RwLock<State> {
    static STATE: OnceLock<RwLock<State>> = OnceLock::new();
    STATE.get_or_init(|| RwLock::new(State { secret: None, extra: Vec::new() }))
}

/// Install the stable noise secret of the active browser profile (`None` —
/// back to the per-run random secret). Process-global like the navigator
/// profile: the next page / worker of that profile picks it up.
pub fn set_noise_secret(secret: Option<u64>) {
    state().write().unwrap_or_else(|e| e.into_inner()).secret = secret;
}

/// Replace the user-supplied exempt hosts (`fingerprint.toml`
/// `noise_exceptions`). Entries are matched as domain suffixes; case and a
/// leading `.` / `*.` are ignored.
pub fn set_noise_exceptions(hosts: &[String]) {
    let extra = hosts
        .iter()
        .map(|h| h.trim().trim_start_matches("*.").trim_start_matches('.').to_ascii_lowercase())
        .filter(|h| !h.is_empty())
        .collect();
    state().write().unwrap_or_else(|e| e.into_inner()).extra = extra;
}

/// The per-run random secret: wall-clock nanoseconds mixed with the pid.
fn run_secret() -> u64 {
    static SECRET: OnceLock<u64> = OnceLock::new();
    *SECRET.get_or_init(|| {
        use std::time::{SystemTime, UNIX_EPOCH};
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        nanos ^ u64::from(std::process::id()).wrapping_mul(0x9E37_79B9_7F4A_7C15)
    })
}

fn fnv1a(mut h: u64, bytes: &[u8]) -> u64 {
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// `host` equals `entry` or is a sub-domain of it.
fn host_matches(host: &str, entry: &str) -> bool {
    host == entry
        || host
            .strip_suffix(entry)
            .is_some_and(|rest| rest.ends_with('.'))
}

/// Whether `host` (lower-case) is exempt from fingerprint noise.
pub(crate) fn is_exempt_host(host: &str) -> bool {
    let guard = state().read().unwrap_or_else(|e| e.into_inner());
    BUILTIN_EXEMPT.iter().any(|e| host_matches(host, e))
        || guard.extra.iter().any(|e| host_matches(host, e))
}

/// The seed for a document of `origin` (`scheme://host[:port]`), or `None`
/// when the site is exempt and no noise must be applied.
pub(crate) fn noise_key(origin: &str) -> Option<u64> {
    let (site, host) = crate::origin::site_and_host_of_origin(origin);
    if is_exempt_host(&host) {
        return None;
    }
    let secret = state().read().unwrap_or_else(|e| e.into_inner()).secret.unwrap_or_else(run_secret);
    Some(fnv1a(0xcbf2_9ce4_8422_2325 ^ secret, site.as_bytes()))
}

/// The secret and the exception list are process-global: every test that
/// sets or depends on them holds this lock.
#[cfg(test)]
pub(crate) static TEST_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    fn guard() -> std::sync::MutexGuard<'static, ()> {
        TEST_GUARD.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn key_follows_secret_and_site_only() {
        let _g = guard();
        set_noise_secret(Some(7));
        let a = noise_key("https://example.test");
        assert!(a.is_some());
        assert_eq!(a, noise_key("https://example.test"), "stable for one secret");
        assert_eq!(a, noise_key("https://example.test:8443"), "the port is not part of the site");
        assert_ne!(a, noise_key("https://other.test"), "sites differ");
        set_noise_secret(Some(8));
        assert_ne!(a, noise_key("https://example.test"), "profiles differ");
        set_noise_secret(None);
    }

    #[test]
    fn per_run_secret_is_stable_within_a_run() {
        let _g = guard();
        set_noise_secret(None);
        assert_eq!(noise_key("https://example.test"), noise_key("https://example.test"));
    }

    #[test]
    fn user_exceptions_are_suffix_matched() {
        let _g = guard();
        set_noise_exceptions(&["*.Bank.Example".to_string(), " shop.test ".to_string()]);
        assert_eq!(noise_key("https://login.bank.example"), None);
        assert_eq!(noise_key("https://shop.test"), None);
        assert!(noise_key("https://notbank.example").is_some());
        set_noise_exceptions(&[]);
        assert!(noise_key("https://shop.test").is_some());
    }

    #[test]
    fn suffix_match_needs_a_label_boundary() {
        assert!(host_matches("www.gov.ru", "gov.ru"));
        assert!(host_matches("gov.ru", "gov.ru"));
        assert!(!host_matches("evilgov.ru", "gov.ru"));
        assert!(host_matches("a.b.sberbank.ru", "sberbank.ru"));
    }

    #[test]
    fn builtin_exempt_covers_checks_banks_and_state() {
        for h in ["www.gosuslugi.ru", "online.sberbank.ru", "challenges.cloudflare.com", "x.gov"] {
            assert!(is_exempt_host(h), "{h}");
        }
        assert!(!is_exempt_host("example.com"));
    }
}
