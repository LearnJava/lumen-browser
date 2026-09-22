//! Blocking cert-error interstitial (ph3-tls-hardening A6).
//!
//! State layer only — mirrors the `PermissionPanel`/`CertPanel` split
//! between panel state (this file) and the engine-rendered chrome DOM that
//! actually paints it (`crates/chrome`, driven by `assets/chrome/chrome.html`
//! and `ChromeAction` dispatch in `chrome_ui.rs`, CC-10 style). This slice
//! wires the *state*: a real `Error::CertInvalid` navigation failure opens
//! this panel (`user_event.rs`'s `LoadEvent::CertError` handler) instead of
//! silently falling into the generic `LoadError` path, and [`proceed`] is
//! the seam a click/keyboard action calls to record the "Proceed anyway"
//! decision. A dedicated `#certInterstitial` chrome asset + `data-action`
//! wiring (the actual "Your connection is not private" screen the design
//! renders) is a follow-up CC design-asset slice — the same split already
//! true of `cert_panel.rs`'s revocation/CT rows (see its module doc).

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
}

impl CertInterstitial {
    /// Create a new, hidden interstitial.
    pub fn new() -> Self {
        Self::default()
    }

    /// Show the interstitial for a failed navigation to `url`/`host`.
    pub fn open(&mut self, url: String, host: String, error: lumen_core::error::CertError) {
        self.url = url;
        self.host = host;
        self.error = Some(error);
        self.visible = true;
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
        if !self.visible {
            return None;
        }
        self.visible = false;
        Some((std::mem::take(&mut self.url), self.host.clone()))
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
        i.open("https://bad.example/".to_owned(), "bad.example".to_owned(), CertError::Expired);
        assert!(i.visible);
        assert_eq!(i.host, "bad.example");
        assert_eq!(i.reason_text().as_deref(), Some("certificate expired"));
    }

    #[test]
    fn close_hides_without_clearing_reason() {
        let mut i = CertInterstitial::new();
        i.open("https://bad.example/".to_owned(), "bad.example".to_owned(), CertError::SelfSigned);
        i.close();
        assert!(!i.visible);
    }

    #[test]
    fn proceed_returns_url_and_host_and_hides() {
        let mut i = CertInterstitial::new();
        i.open("https://bad.example/path".to_owned(), "bad.example".to_owned(), CertError::Untrusted("x".to_owned()));
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
    fn reason_text_none_when_never_opened() {
        let i = CertInterstitial::new();
        assert_eq!(i.reason_text(), None);
    }
}
