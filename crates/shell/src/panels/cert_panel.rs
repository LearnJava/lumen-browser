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

/// Panel width in CSS px (exported for anchor calculation in main.rs).
pub const PANEL_W: f32 = 500.0;
/// Panel height in CSS px (exported for anchor calculation in main.rs).
pub const PANEL_H: f32 = 440.0;
/// Header bar height.
const HEADER_H: f32 = 36.0;
/// Height of one data row.
const ROW_H: f32 = 36.0;
/// Width of the × close button hit zone.
const CLOSE_W: f32 = 30.0;
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
}

impl PanelCertData {
    /// Returns `true` if there is meaningful data to display.
    pub fn has_data(&self) -> bool {
        !self.subject_cn.is_empty()
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

    /// Hit-test a pointer position relative to panel origin.
    ///
    /// Returns `CertHit` describing which element was hit.
    pub fn hit_test(&self, lx: f32, ly: f32) -> CertHit {
        if ly < HEADER_H {
            if lx >= PANEL_W - CLOSE_W {
                return CertHit::Close;
            }
            return CertHit::Header;
        }
        CertHit::Body
    }
}

/// Result of a pointer hit test on the cert panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CertHit {
    /// User clicked the × close button.
    Close,
    /// User clicked the panel header bar (drag area — future use).
    Header,
    /// User clicked inside the scrollable body.
    Body,
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
        }
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
    fn cert_panel_hit_test_close() {
        let p = CertPanel::new();
        let hit = p.hit_test(PANEL_W - 5.0, HEADER_H * 0.5);
        assert_eq!(hit, CertHit::Close);
    }

    #[test]
    fn cert_panel_hit_test_header() {
        let p = CertPanel::new();
        let hit = p.hit_test(10.0, HEADER_H * 0.5);
        assert_eq!(hit, CertHit::Header);
    }

    #[test]
    fn cert_panel_hit_test_body() {
        let p = CertPanel::new();
        let hit = p.hit_test(50.0, HEADER_H + 10.0);
        assert_eq!(hit, CertHit::Body);
    }

    #[test]
    fn panel_cert_data_has_data() {
        let cert = sample_cert();
        assert!(cert.has_data());
        let empty = PanelCertData::default();
        assert!(!empty.has_data());
    }

    #[test]
    fn build_rows_has_nine_entries() {
        let cert = sample_cert();
        let rows = build_rows(&cert);
        assert_eq!(rows.len(), 9);
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
