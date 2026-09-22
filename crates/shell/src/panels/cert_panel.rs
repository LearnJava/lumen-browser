//! Certificate viewer panel (D-1).
//!
//! A centred overlay (500 × 440 px) opened by `Ctrl+Shift+C`.
//! Displays TLS/X.509 certificate information for the current page:
//! subject CN and Organisation, issuer, validity period, SHA-256
//! fingerprint, Subject Alternative Names, and the negotiated TLS version.
//!
//! When no certificate information is available (HTTP or Phase 0 stub)
//! the panel shows a single "No certificate information" row.

// ── Geometry ─────────────────────────────────────────────────────────────────

/// Panel height in CSS px (exported for anchor calculation in main.rs).
pub const PANEL_H: f32 = 440.0;
/// Header bar height.
const HEADER_H: f32 = 36.0;
/// Height of one data row.
const ROW_H: f32 = 36.0;
/// Visible content area height.
const CONTENT_H: f32 = PANEL_H - HEADER_H;

// ── Data ─────────────────────────────────────────────────────────────────────

/// Certificate data shown in the panel.
///
/// Mirrors `lumen_network::CertInfo` but owned by the panel to avoid coupling
/// the panel crate to `lumen-network` directly.  Shell copies fields on open.
#[derive(Debug, Clone, Default)]
pub struct PanelCertData {
    /// Subject Common Name (e.g. `"example.com"`).
    pub subject_cn: String,
    /// Subject Organisation (may be empty).
    pub subject_org: String,
    /// Issuer Common Name (e.g. `"Let's Encrypt Authority X3"`).
    pub issuer_cn: String,
    /// Issuer Organisation.
    pub issuer_org: String,
    /// Validity start (ISO 8601 string, may be empty).
    pub not_before: String,
    /// Validity end (ISO 8601 string, may be empty).
    pub not_after: String,
    /// Hex SHA-256 fingerprint, colon-separated bytes.
    pub fingerprint_sha256: String,
    /// Subject Alternative Names (DNS only).
    pub san_list: Vec<String>,
    /// Human-readable TLS protocol version (e.g. `"TLS 1.3"`).
    pub tls_version: String,
    /// Revocation status (A3, stapled OCSP) — e.g. `"good"`, `"no OCSP staple"`.
    pub revocation_status: String,
    /// Certificate Transparency status (A4) — e.g. `"2 SCTs (sufficient)"`.
    pub ct_status: String,
    /// A6: human-readable reason the connection was rejected, when the page
    /// currently shown was reached only via "Proceed anyway" on the cert
    /// interstitial (`cert_interstitial.rs`). `None` for an ordinary,
    /// trusted connection — the panel's usual green/passive state.
    pub error: Option<String>,
}

impl PanelCertData {
    /// Returns `true` if there is meaningful data to display.
    pub fn has_data(&self) -> bool {
        !self.subject_cn.is_empty()
    }

    /// Returns `true` when this cert was accepted only via an explicit user
    /// override (A6) — the panel should render its red/warning header.
    pub fn has_error(&self) -> bool {
        self.error.is_some()
    }
}

/// Copy the fields this panel renders out of a real `lumen_network::CertInfo`
/// (ph3-tls-hardening, live-wiring slice + A6). `error` has no `CertInfo`
/// counterpart — it is stamped separately by the cert-interstitial "Proceed
/// anyway" flow (`cert_interstitial.rs`), never derived from a successful
/// handshake's `CertInfo`.
impl From<lumen_network::CertInfo> for PanelCertData {
    fn from(info: lumen_network::CertInfo) -> Self {
        Self {
            subject_cn: info.subject_cn,
            subject_org: info.subject_org,
            issuer_cn: info.issuer_cn,
            issuer_org: info.issuer_org,
            not_before: info.not_before,
            not_after: info.not_after,
            fingerprint_sha256: info.fingerprint_sha256,
            san_list: info.san_list,
            tls_version: info.tls_version,
            revocation_status: info.revocation_status,
            ct_status: info.ct_status,
            error: None,
        }
    }
}

// ── State ─────────────────────────────────────────────────────────────────────

/// Certificate viewer panel state.
#[derive(Debug, Default)]
pub struct CertPanel {
    /// Whether the panel is currently shown.
    pub visible: bool,
    /// Certificate data for the currently displayed page. `None` = HTTP or no info.
    pub cert: Option<PanelCertData>,
    /// Vertical scroll offset (CSS px, clamped to content height).
    pub scroll_y: f32,
}

impl CertPanel {
    /// Create a new, hidden panel.
    pub fn new() -> Self {
        Self::default()
    }

    /// Open the panel with the given certificate data.
    ///
    /// Pass `None` when the current page is HTTP or no cert info is available.
    pub fn open(&mut self, cert: Option<PanelCertData>) {
        self.cert = cert;
        self.scroll_y = 0.0;
        self.visible = true;
    }

    /// Close the panel.
    pub fn close(&mut self) {
        self.visible = false;
    }

    /// Toggle visibility.  On open: resets scroll to top.
    pub fn toggle(&mut self, cert: Option<PanelCertData>) {
        if self.visible {
            self.close();
        } else {
            self.open(cert);
        }
    }

    /// Scroll the content by `delta` CSS px (positive = down).
    pub fn scroll_by(&mut self, delta: f32) {
        let max = content_scroll_max(self.cert.as_ref());
        self.scroll_y = (self.scroll_y + delta).clamp(0.0, max);
    }

}

// ── Rows ──────────────────────────────────────────────────────────────────────

/// Flat list of (label, value) pairs to render.
///
/// SAN list is joined with commas; long values are truncated in the renderer.
fn build_rows(cert: &PanelCertData) -> Vec<(&'static str, String)> {
    let san_str = if cert.san_list.is_empty() {
        String::from("\u{2014}")
    } else {
        cert.san_list.join(", ")
    };

    let fingerprint = if cert.fingerprint_sha256.is_empty() {
        String::from("\u{2014}")
    } else {
        cert.fingerprint_sha256.clone()
    };

    let not_before = if cert.not_before.is_empty() {
        String::from("\u{2014}")
    } else {
        cert.not_before.clone()
    };

    let not_after = if cert.not_after.is_empty() {
        String::from("\u{2014}")
    } else {
        cert.not_after.clone()
    };

    let revocation = if cert.revocation_status.is_empty() {
        String::from("\u{2014}")
    } else {
        cert.revocation_status.clone()
    };

    let ct_status = if cert.ct_status.is_empty() {
        String::from("\u{2014}")
    } else {
        cert.ct_status.clone()
    };

    vec![
        ("Subject CN",   cert.subject_cn.clone()),
        ("Subject Org",  if cert.subject_org.is_empty() { String::from("\u{2014}") } else { cert.subject_org.clone() }),
        ("Issuer CN",    cert.issuer_cn.clone()),
        ("Issuer Org",   if cert.issuer_org.is_empty() { String::from("\u{2014}") } else { cert.issuer_org.clone() }),
        ("Valid From",   not_before),
        ("Valid Until",  not_after),
        ("TLS Version",  cert.tls_version.clone()),
        ("SANs",         san_str),
        ("SHA-256",      fingerprint),
        ("Revocation",   revocation),
        ("CT Status",    ct_status),
    ]
}

/// Maximum scroll offset in CSS px for the given cert data.
fn content_scroll_max(cert: Option<&PanelCertData>) -> f32 {
    let row_count = match cert {
        Some(c) if c.has_data() => build_rows(c).len() as f32,
        _ => 1.0,
    };
    let total_h = row_count * ROW_H;
    (total_h - CONTENT_H).max(0.0)
}

// ── Unit tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_cert() -> PanelCertData {
        PanelCertData {
            subject_cn: String::from("example.com"),
            subject_org: String::from("Example Org"),
            issuer_cn: String::from("Let's Encrypt Authority X3"),
            issuer_org: String::from("Let's Encrypt"),
            not_before: String::from("2025-01-01T00:00:00Z"),
            not_after: String::from("2026-01-01T00:00:00Z"),
            fingerprint_sha256: String::from("AA:BB:CC:DD:EE:FF:00:11:22:33"),
            san_list: vec![String::from("example.com"), String::from("www.example.com")],
            tls_version: String::from("TLS 1.3"),
            revocation_status: String::from("good"),
            ct_status: String::from("2 SCTs (sufficient)"),
            error: None,
        }
    }

    #[test]
    fn from_cert_info_copies_shared_fields() {
        let info = lumen_network::CertInfo::from_peer_cert(
            &[],
            "TLS 1.3",
            lumen_network::tls::ocsp::OcspVerdict::Unknown,
            lumen_network::tls::ct::CtVerdict::Insufficient(0),
        );
        let data = PanelCertData::from(info.clone());
        assert_eq!(data.subject_cn, info.subject_cn);
        assert_eq!(data.subject_org, info.subject_org);
        assert_eq!(data.issuer_cn, info.issuer_cn);
        assert_eq!(data.issuer_org, info.issuer_org);
        assert_eq!(data.not_before, info.not_before);
        assert_eq!(data.not_after, info.not_after);
        assert_eq!(data.fingerprint_sha256, info.fingerprint_sha256);
        assert_eq!(data.san_list, info.san_list);
        assert_eq!(data.tls_version, "TLS 1.3");
        assert_eq!(data.revocation_status, info.revocation_status);
        assert_eq!(data.ct_status, info.ct_status);
        assert!(data.error.is_none());
    }

    #[test]
    fn has_error_false_for_ordinary_connection() {
        assert!(!sample_cert().has_error());
    }

    #[test]
    fn has_error_true_when_reason_stamped() {
        let mut cert = sample_cert();
        cert.error = Some(String::from("self-signed certificate"));
        assert!(cert.has_error());
    }

    #[test]
    fn cert_panel_default_not_visible() {
        let p = CertPanel::new();
        assert!(!p.visible);
        assert!(p.cert.is_none());
    }

    #[test]
    fn cert_panel_open_sets_visible() {
        let mut p = CertPanel::new();
        p.open(Some(sample_cert()));
        assert!(p.visible);
        assert!(p.cert.is_some());
    }

    #[test]
    fn cert_panel_close_hides_panel() {
        let mut p = CertPanel::new();
        p.open(Some(sample_cert()));
        p.close();
        assert!(!p.visible);
    }

    #[test]
    fn cert_panel_toggle_opens_then_closes() {
        let mut p = CertPanel::new();
        p.toggle(Some(sample_cert()));
        assert!(p.visible);
        p.toggle(None);
        assert!(!p.visible);
    }




    #[test]
    fn panel_cert_data_has_data() {
        let cert = sample_cert();
        assert!(cert.has_data());
        let empty = PanelCertData::default();
        assert!(!empty.has_data());
    }

    #[test]
    fn build_rows_has_eleven_entries() {
        let cert = sample_cert();
        let rows = build_rows(&cert);
        assert_eq!(rows.len(), 11);
    }

    #[test]
    fn scroll_clamped_at_zero() {
        let mut p = CertPanel::new();
        p.open(Some(sample_cert()));
        p.scroll_by(-100.0);
        assert_eq!(p.scroll_y, 0.0);
    }

    #[test]
    fn scroll_clamped_at_max() {
        let mut p = CertPanel::new();
        p.open(Some(sample_cert()));
        p.scroll_by(10_000.0);
        let max = content_scroll_max(p.cert.as_ref());
        assert_eq!(p.scroll_y, max);
    }
}
