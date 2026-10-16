//! Blocking cert-error interstitial (ph3-tls-hardening A6).
//!
//! State layer only — mirrors the `PermissionPanel`/`CertPanel` split
//! between panel state (this file) and the engine-rendered chrome DOM that
//! actually paints it (`crates/chrome`, driven by `assets/chrome/chrome.html`
//! and `ChromeAction` dispatch in `chrome_ui.rs`, CC-10 style). This slice
//! wires the *state*: a real `Error::CertInvalid` navigation failure opens
//! this panel (`user_event.rs`'s `LoadEvent::CertError` handler) instead of
//! silently falling into the generic `LoadError` path, and [`proceed`] is
//! the seam the `#securityBox` "Proceed anyway" button (or Enter) calls to
//! record the decision; [`CertInterstitial::chrome_model`] feeds the screen.

/// Blocking interstitial shown when a navigation's TLS handshake fails cert
/// verification.
#[derive(Debug, Default)]
pub struct CertInterstitial {
    /// `true` while the interstitial is blocking the view.
    pub visible: bool,
    /// Full URL of the navigation that failed (used to retry after "Proceed
    /// anyway" — the same URL the shell's `PageSource` still holds).
    pub url: String,
    /// Hostname the "Proceed anyway" override is scoped to
    /// (`lumen_network::tls::bypass::allow_host`'s key).
    pub host: String,
    /// Structured failure reason, for [`Self::reason_text`].
    pub error: Option<lumen_core::error::CertError>,
    /// Угроза из локальной базы Safe Browsing (UX-SECURITY-UI, срез 2):
    /// `(имя списка, тип)`. `Some` — экран показывает предупреждение о сайте, а не
    /// ошибку сертификата.
    pub threat: Option<(String, lumen_storage::ThreatType)>,
    /// `true` when the failed host is a Known HSTS Host (RFC 6797 §8.4/§12.1:
    /// the connection MUST be terminated, no user override) — BUG-1615.
    pub hsts_host: bool,
    /// Diagnosed cause of the error (COMPAT-TRUST-CAUSES): which headline and
    /// advice the screen shows instead of the generic warning.
    pub cause: crate::cert_cause::CertCause,
}

impl CertInterstitial {
    /// Create a new, hidden interstitial.
    pub fn new() -> Self {
        Self::default()
    }

    /// Show the interstitial for a failed navigation to `url`/`host`.
    ///
    /// `hsts_host`: the host is a Known HSTS Host (preload list or a learned
    /// policy) — see [`Self::can_proceed`].
    #[cfg(test)]
    pub fn open(
        &mut self,
        url: String,
        host: String,
        error: lumen_core::error::CertError,
        hsts_host: bool,
    ) {
        self.open_with_cause(url, host, error, hsts_host, crate::cert_cause::CertCause::Unexplained);
    }

    /// [`Self::open`] with the diagnosed `cause` of the error.
    pub fn open_with_cause(
        &mut self,
        url: String,
        host: String,
        error: lumen_core::error::CertError,
        hsts_host: bool,
        cause: crate::cert_cause::CertCause,
    ) {
        self.url = url;
        self.host = host;
        self.error = Some(error);
        self.threat = None;
        self.hsts_host = hsts_host;
        self.cause = cause;
        self.visible = true;
    }

    /// Показать предупреждение о сайте из базы Safe Browsing.
    pub fn open_threat(&mut self, url: String, host: String, list: String, threat: lumen_storage::ThreatType) {
        self.url = url;
        self.host = host;
        self.error = None;
        self.threat = Some((list, threat));
        self.hsts_host = false;
        self.cause = crate::cert_cause::CertCause::Unexplained;
        self.visible = true;
    }

    /// Whether the "Proceed anyway" action may be offered and honoured.
    /// Never on a Known HSTS Host (RFC 6797 §8.4/§12.1, ADR-034 §3) and never
    /// for a revoked certificate (BUG-1615).
    pub fn can_proceed(&self) -> bool {
        !self.hsts_host && !matches!(self.error, Some(lumen_core::error::CertError::Revoked))
    }

    /// Whether the primary button is the consent to the Russian state CA for this host
    /// (COMPAT-TRUST-NUC) rather than a bypass: the chain leads to that CA and the user has
    /// not agreed yet. Allowed on HSTS hosts too — the certificate is still fully verified and
    /// must carry CT evidence, nothing is waived.
    pub fn offers_nuc_consent(&self) -> bool {
        self.visible
            && self.threat.is_none()
            && matches!(self.cause, crate::cert_cause::CertCause::GovCa)
            && !lumen_network::tls::nuc::has_consent(&self.host)
    }

    /// The consent button was pressed: close the screen and return the host to trust the
    /// national CA for. `None` when this screen does not offer it.
    pub fn take_nuc_consent(&mut self) -> Option<String> {
        if !self.offers_nuc_consent() {
            return None;
        }
        self.visible = false;
        Some(self.host.clone())
    }

    /// Dismiss without proceeding ("Back" / navigate away).
    pub fn close(&mut self) {
        self.visible = false;
    }

    /// "Proceed anyway": consume the interstitial and return `(url, host)`
    /// for the caller to record a bypass override
    /// (`lumen_network::tls::bypass::allow_host`) and retry the navigation.
    /// Returns `None` if the interstitial isn't currently showing anything
    /// to proceed past.
    pub fn proceed(&mut self) -> Option<(String, String)> {
        if !self.visible || !self.can_proceed() {
            return None;
        }
        self.visible = false;
        Some((std::mem::take(&mut self.url), self.host.clone()))
    }

    /// Модель экрана `#securityBox` (UX-SECURITY-UI); скрытый экран — по умолчанию.
    pub fn chrome_model(&self) -> lumen_chrome::ChromeSecurityModel {
        if !self.visible {
            return lumen_chrome::ChromeSecurityModel::default();
        }
        if let Some((list, threat)) = &self.threat {
            let (title, message) = crate::threat_store::describe(threat, &self.host);
            return lumen_chrome::ChromeSecurityModel {
                open: true,
                title,
                message,
                detail: format!("{} ({list})", threat.as_code()),
                proceed_label: "Всё равно перейти".to_owned(),
                other_browser: false,
                proceed_blocked: false,
                time_settings: false,
            };
        }
        // Unexplained errors keep the warning about interception; an explained
        // one (clock, antivirus, portal, ...) says what actually happened.
        let consent = self.offers_nuc_consent();
        let gov_ca = matches!(self.cause, crate::cert_cause::CertCause::GovCa);
        let described = if gov_ca {
            Some(crate::cert_cause::gov_ca_text(&self.host, !consent))
        } else {
            self.cause.describe(&self.host)
        };
        let (title, message) = described.unwrap_or_else(|| {
            (
                "Ваше подключение не защищено".to_owned(),
                format!(
                    "Сертификат сайта {} не прошёл проверку. Злоумышленники могут перехватить пароли и другие данные.",
                    self.host
                ),
            )
        });
        lumen_chrome::ChromeSecurityModel {
            open: true,
            title,
            message,
            detail: self.detail_text(),
            proceed_label: if consent { "Доверять центру Минцифры для этого сайта" } else { "Всё равно перейти" }
                .to_owned(),
            other_browser: self.handoff_reason().is_some(),
            proceed_blocked: !consent && !self.can_proceed(),
            time_settings: self.cause.offers_time_settings(),
        }
    }

    /// Small print at the bottom of the screen: Chromium's code
    /// (`ERR_CERT_DATE_INVALID`, ...) and the technical reason, so the
    /// instructions written against those codes apply.
    fn detail_text(&self) -> String {
        match &self.error {
            Some(error) => format!("{} · {error}", error.chromium_code()),
            None => String::new(),
        }
    }

    /// Причина передачи во внешний браузер (COMPAT-HO-02), если она есть:
    /// цепочка не ведёт к доверенному корню — корень НУЦ ставят в ОС, и другой
    /// браузер его видит. Просроченный, отозванный, самоподписанный сертификат
    /// и чужое имя там дадут ту же ошибку; угроза Safe Browsing — не повод
    /// обходить предупреждение.
    pub fn handoff_reason(&self) -> Option<crate::external_browser::HandoffReason> {
        match self.error {
            Some(lumen_core::error::CertError::Untrusted(_)) => {
                Some(crate::external_browser::HandoffReason::Crypto)
            }
            _ => None,
        }
    }

    /// Human-readable one-line reason, for the interstitial body text and
    /// for stamping onto `PanelCertData::error`.
    pub fn reason_text(&self) -> Option<String> {
        self.error.as_ref().map(std::string::ToString::to_string)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::error::CertError;

    #[test]
    fn new_interstitial_hidden() {
        let i = CertInterstitial::new();
        assert!(!i.visible);
        assert!(i.error.is_none());
    }

    #[test]
    fn open_shows_and_stores_reason() {
        let mut i = CertInterstitial::new();
        i.open("https://bad.example/".to_owned(), "bad.example".to_owned(), CertError::Expired, false);
        assert!(i.visible);
        assert_eq!(i.host, "bad.example");
        assert_eq!(i.reason_text().as_deref(), Some("certificate expired"));
    }

    #[test]
    fn close_hides_without_clearing_reason() {
        let mut i = CertInterstitial::new();
        i.open("https://bad.example/".to_owned(), "bad.example".to_owned(), CertError::SelfSigned, false);
        i.close();
        assert!(!i.visible);
    }

    #[test]
    fn proceed_returns_url_and_host_and_hides() {
        let mut i = CertInterstitial::new();
        i.open("https://bad.example/path".to_owned(), "bad.example".to_owned(), CertError::Untrusted("x".to_owned()), false);
        let result = i.proceed();
        assert_eq!(result, Some(("https://bad.example/path".to_owned(), "bad.example".to_owned())));
        assert!(!i.visible);
    }

    #[test]
    fn proceed_on_hidden_interstitial_returns_none() {
        let mut i = CertInterstitial::new();
        assert_eq!(i.proceed(), None);
    }

    #[test]
    fn chrome_model_is_open_only_while_visible() {
        let mut i = CertInterstitial::new();
        assert!(!i.chrome_model().open);
        i.open("https://bad.example/".to_owned(), "bad.example".to_owned(), CertError::Expired, false);
        let m = i.chrome_model();
        assert!(m.open);
        assert!(m.message.contains("bad.example"));
        assert_eq!(m.detail, "ERR_CERT_DATE_INVALID · certificate expired");
        i.close();
        assert!(!i.chrome_model().open);
    }

    #[test]
    fn other_browser_offered_only_for_untrusted_chain() {
        use crate::external_browser::HandoffReason;
        let mut i = CertInterstitial::new();
        i.open("https://nuc.example/".to_owned(), "nuc.example".to_owned(), CertError::Untrusted("UnknownIssuer".to_owned()), false);
        assert_eq!(i.handoff_reason(), Some(HandoffReason::Crypto));
        assert!(i.chrome_model().other_browser);
        for err in [CertError::Expired, CertError::SelfSigned, CertError::NameMismatch, CertError::Revoked] {
            i.open("https://bad.example/".to_owned(), "bad.example".to_owned(), err, false);
            assert_eq!(i.handoff_reason(), None);
            assert!(!i.chrome_model().other_browser);
        }
        i.open_threat(
            "https://phish.example/".to_owned(),
            "phish.example".to_owned(),
            "local".to_owned(),
            lumen_storage::ThreatType::SocialEngineering,
        );
        assert!(!i.chrome_model().other_browser);
    }

    #[test]
    fn state_ca_error_offers_consent_instead_of_a_bypass() {
        use crate::cert_cause::CertCause;
        let host = "bank.interstitial-test.example";
        lumen_network::tls::nuc::revoke_host(host);
        let mut i = CertInterstitial::new();
        let err = CertError::Untrusted("UnknownIssuer".to_owned());
        // A Known HSTS Host cannot be bypassed, but the consent is not a bypass.
        i.open_with_cause(format!("https://{host}/"), host.to_owned(), err, true, CertCause::GovCa);
        assert!(!i.can_proceed());
        assert!(i.offers_nuc_consent());
        let m = i.chrome_model();
        assert!(m.proceed_label.contains("Минцифры") && !m.proceed_blocked, "{m:?}");
        assert_eq!(i.take_nuc_consent().as_deref(), Some(host));
        assert!(!i.visible);
    }

    #[test]
    fn state_ca_consent_is_not_offered_twice_or_for_other_causes() {
        use crate::cert_cause::CertCause;
        let host = "again.interstitial-test.example";
        let err = || CertError::Untrusted("UnknownIssuer".to_owned());
        let mut i = CertInterstitial::new();
        i.open_with_cause(format!("https://{host}/"), host.to_owned(), err(), false, CertCause::IncompleteChain);
        assert!(!i.offers_nuc_consent() && i.take_nuc_consent().is_none());
        // Already consented but the chain still fails: the ordinary screen with its bypass.
        lumen_network::tls::nuc::consent_host(host);
        i.open_with_cause(format!("https://{host}/"), host.to_owned(), err(), false, CertCause::GovCa);
        assert!(!i.offers_nuc_consent());
        let m = i.chrome_model();
        assert_eq!(m.proceed_label, "Всё равно перейти");
        assert!(m.title.contains("не прошёл проверку"), "{}", m.title);
        lumen_network::tls::nuc::revoke_host(host);
    }

    #[test]
    fn threat_screen_names_phishing_and_clears_on_cert_open() {
        let mut i = CertInterstitial::new();
        i.open_threat(
            "https://phish.example/".to_owned(),
            "phish.example".to_owned(),
            "local".to_owned(),
            lumen_storage::ThreatType::SocialEngineering,
        );
        let m = i.chrome_model();
        assert!(m.open);
        assert!(m.title.contains("фишинг"));
        assert_eq!(m.detail, "social_engineering (local)");
        i.open("https://bad.example/".to_owned(), "bad.example".to_owned(), CertError::Expired, false);
        assert!(i.threat.is_none());
        assert_eq!(i.chrome_model().detail, "ERR_CERT_DATE_INVALID · certificate expired");
    }

    #[test]
    fn reason_text_none_when_never_opened() {
        let i = CertInterstitial::new();
        assert_eq!(i.reason_text(), None);
    }

    #[test]
    fn hsts_host_cannot_proceed() {
        let mut i = CertInterstitial::new();
        i.open("https://hsts.example/".to_owned(), "hsts.example".to_owned(), CertError::SelfSigned, true);
        assert!(!i.can_proceed());
        assert_eq!(i.proceed(), None);
        assert!(i.visible, "interstitial stays up: there is nothing to proceed to");
    }

    #[test]
    fn chrome_model_hides_proceed_button_when_blocked() {
        let mut i = CertInterstitial::new();
        i.open("https://hsts.example/".to_owned(), "hsts.example".to_owned(), CertError::SelfSigned, true);
        assert!(i.chrome_model().proceed_blocked);
        i.open("https://bad.example/".to_owned(), "bad.example".to_owned(), CertError::Expired, false);
        assert!(!i.chrome_model().proceed_blocked);
    }

    #[test]
    fn revoked_cert_cannot_proceed() {
        let mut i = CertInterstitial::new();
        i.open("https://bad.example/".to_owned(), "bad.example".to_owned(), CertError::Revoked, false);
        assert!(!i.can_proceed());
        assert_eq!(i.proceed(), None);
    }

    #[test]
    fn ordinary_error_on_plain_host_can_proceed() {
        let mut i = CertInterstitial::new();
        i.open("https://bad.example/".to_owned(), "bad.example".to_owned(), CertError::Expired, false);
        assert!(i.can_proceed());
    }

    #[test]
    fn unexplained_error_keeps_the_generic_warning() {
        let mut i = CertInterstitial::new();
        i.open("https://bad.example/".to_owned(), "bad.example".to_owned(), CertError::SelfSigned, false);
        let m = i.chrome_model();
        assert_eq!(m.title, "Ваше подключение не защищено");
        assert!(m.message.contains("Злоумышленники"));
        assert!(!m.time_settings);
        assert!(m.detail.starts_with("ERR_CERT_AUTHORITY_INVALID"));
    }

    #[test]
    fn diagnosed_cause_replaces_the_generic_warning() {
        use crate::cert_cause::CertCause;
        let mut i = CertInterstitial::new();
        let clock = CertCause::WrongClock { system_unix: 1_791_979_200, reference_unix: None };
        i.open_with_cause("https://a.example/".to_owned(), "a.example".to_owned(), CertError::NotValidYet, false, clock);
        let m = i.chrome_model();
        assert!(m.title.contains("дата"));
        assert!(m.message.contains("14.10.2026"));
        assert!(!m.message.contains("Злоумышленники"));
        assert!(m.time_settings);
        assert_eq!(m.detail, "ERR_CERT_DATE_INVALID · certificate not yet valid");

        let av = CertCause::Antivirus { product: "Kaspersky" };
        i.open_with_cause("https://b.example/".to_owned(), "b.example".to_owned(), CertError::Untrusted("x".to_owned()), false, av);
        let m = i.chrome_model();
        assert!(m.title.contains("Kaspersky"));
        assert!(!m.time_settings);

        // A later error without a diagnosis must not inherit the old cause.
        i.open("https://c.example/".to_owned(), "c.example".to_owned(), CertError::Expired, false);
        assert_eq!(i.cause, CertCause::Unexplained);
    }

    #[test]
    fn revoked_screen_names_the_ca_and_has_no_proceed() {
        use crate::cert_cause::CertCause;
        let mut i = CertInterstitial::new();
        i.open_with_cause("https://r.example/".to_owned(), "r.example".to_owned(), CertError::Revoked, false, CertCause::Revoked);
        let m = i.chrome_model();
        assert_eq!(m.title, "Сертификат отозван");
        assert!(m.proceed_blocked);
        assert!(m.detail.starts_with("ERR_CERT_REVOKED"));
    }
}
