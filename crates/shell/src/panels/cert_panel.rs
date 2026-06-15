//! Certificate viewer panel (D-1).
//!
//! A centred overlay (500 × 440 px) opened by `Ctrl+Shift+C`.
//! Displays TLS/X.509 certificate information for the current page:
//! subject CN and Organisation, issuer, validity period, SHA-256
//! fingerprint, Subject Alternative Names, and the negotiated TLS version.
//!
//! When no certificate information is available (HTTP or Phase 0 stub)
//! the panel shows a single "No certificate information" row.

use lumen_core::geom::Rect;
use lumen_layout::{Color, FontStyle, FontWeight};
use lumen_paint::{CornerRadii, DisplayCommand};

type DisplayList = Vec<DisplayCommand>;

// ── Geometry ─────────────────────────────────────────────────────────────────

/// Panel width in CSS px (exported for anchor calculation in main.rs).
pub const PANEL_W: f32 = 500.0;
/// Panel height in CSS px (exported for anchor calculation in main.rs).
pub const PANEL_H: f32 = 440.0;
/// Header bar height.
const HEADER_H: f32 = 36.0;
/// Height of one data row.
const ROW_H: f32 = 36.0;
/// Left padding for label text inside each row.
const PAD_H: f32 = 14.0;
/// Column break between label and value (label occupies 0..LABEL_COL_W).
const LABEL_COL_W: f32 = 150.0;
/// Width of the × close button hit zone.
const CLOSE_W: f32 = 30.0;
/// Visible content area height.
const CONTENT_H: f32 = PANEL_H - HEADER_H;

// ── Colours ──────────────────────────────────────────────────────────────────

const PANEL_BG: Color = Color { r: 18, g: 18, b: 26, a: 254 };
const PANEL_BORDER: Color = Color { r: 52, g: 52, b: 66, a: 255 };
const HEADER_BG: Color = Color { r: 26, g: 26, b: 36, a: 255 };
const HEADER_TEXT: Color = Color { r: 210, g: 210, b: 225, a: 255 };
const CLOSE_COL: Color = Color { r: 180, g: 80, b: 80, a: 255 };
const ROW_EVEN: Color = Color { r: 22, g: 22, b: 32, a: 255 };
const ROW_ODD: Color = Color { r: 26, g: 26, b: 36, a: 255 };
const LABEL_COL: Color = Color { r: 140, g: 160, b: 200, a: 255 };
const VALUE_COL: Color = Color { r: 200, g: 200, b: 218, a: 255 };
const SEPARATOR: Color = Color { r: 36, g: 36, b: 50, a: 255 };
const SECURE_GREEN: Color = Color { r: 60, g: 180, b: 100, a: 255 };
const NO_CERT_COL: Color = Color { r: 160, g: 120, b: 80, a: 255 };

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

// ── Render helper ─────────────────────────────────────────────────────────────

fn txt(
    text: impl Into<String>,
    x: f32,
    y: f32,
    w: f32,
    font_size: f32,
    font_weight: FontWeight,
    color: Color,
) -> DisplayCommand {
    DisplayCommand::DrawText {
        rect: Rect::new(x, y, w, font_size * 1.4),
        text: text.into(),
        font_size,
        color,
        font_family: Vec::new(),
        font_weight,
        font_style: FontStyle::Normal,
        font_variation_axes: Vec::new(),
        tab_size: 0.0,
        highlight_name: None,
    }
}

// ── Rendering ─────────────────────────────────────────────────────────────────

/// Append display commands for the cert panel to `buf`.
///
/// `px`, `py` — top-left panel origin in CSS px (window-space).
pub fn build_panel(panel: &CertPanel, buf: &mut DisplayList, px: f32, py: f32) {
    // Panel background + border.
    buf.push(DisplayCommand::FillRoundedRect {
        rect: Rect::new(px, py, PANEL_W, PANEL_H),
        radii: CornerRadii { tl: 6.0, tl_y: 6.0, tr: 6.0, tr_y: 6.0, bl: 6.0, bl_y: 6.0, br: 6.0, br_y: 6.0 },
        color: PANEL_BORDER,
    });
    buf.push(DisplayCommand::FillRoundedRect {
        rect: Rect::new(px + 1.0, py + 1.0, PANEL_W - 2.0, PANEL_H - 2.0),
        radii: CornerRadii { tl: 5.0, tl_y: 5.0, tr: 5.0, tr_y: 5.0, bl: 5.0, bl_y: 5.0, br: 5.0, br_y: 5.0 },
        color: PANEL_BG,
    });

    // ── Header ────────────────────────────────────────────────────────────────
    buf.push(DisplayCommand::FillRoundedRect {
        rect: Rect::new(px, py, PANEL_W, HEADER_H),
        radii: CornerRadii { tl: 5.0, tl_y: 5.0, tr: 5.0, tr_y: 5.0, bl: 0.0, bl_y: 0.0, br: 0.0, br_y: 0.0 },
        color: HEADER_BG,
    });
    // Lock icon (unicode padlock) + title.
    let lock = if panel.cert.as_ref().is_some_and(|c| c.has_data()) { "\u{1F512} " } else { "\u{1F513} " };
    let title = format!("{lock}Certificate Information");
    buf.push(txt(title, px + PAD_H, py + HEADER_H * 0.5 - 7.0,
        PANEL_W - CLOSE_W - PAD_H * 2.0, 13.0, FontWeight::BOLD, HEADER_TEXT));
    // Close button ×.
    buf.push(txt("\u{00D7}", px + PANEL_W - CLOSE_W + 6.0, py + HEADER_H * 0.5 - 8.0,
        CLOSE_W, 18.0, FontWeight::BOLD, CLOSE_COL));

    // Separator line below header.
    buf.push(DisplayCommand::FillRect {
        rect: Rect::new(px, py + HEADER_H, PANEL_W, 1.0),
        color: SEPARATOR,
    });

    // ── Clip body ─────────────────────────────────────────────────────────────
    buf.push(DisplayCommand::PushClipRect {
        rect: Rect::new(px, py + HEADER_H, PANEL_W, CONTENT_H),
    });

    let scroll = panel.scroll_y;
    let body_top = py + HEADER_H - scroll;

    match &panel.cert {
        Some(cert) if cert.has_data() => {
            let rows = build_rows(cert);
            for (i, (label, value)) in rows.iter().enumerate() {
                let ry = body_top + i as f32 * ROW_H;
                let row_bg = if i % 2 == 0 { ROW_EVEN } else { ROW_ODD };
                buf.push(DisplayCommand::FillRect {
                    rect: Rect::new(px, ry, PANEL_W, ROW_H),
                    color: row_bg,
                });

                // Label.
                buf.push(txt(*label, px + PAD_H, ry + ROW_H * 0.5 - 7.0,
                    LABEL_COL_W - PAD_H, 12.0, FontWeight::NORMAL, LABEL_COL));

                // Value — truncate long fingerprints.
                let value_text = truncate_value(value, 38);
                let value_color = if *label == "Subject CN" { SECURE_GREEN } else { VALUE_COL };
                buf.push(txt(value_text, px + LABEL_COL_W, ry + ROW_H * 0.5 - 7.0,
                    PANEL_W - LABEL_COL_W - PAD_H, 11.5, FontWeight::NORMAL, value_color));

                // Row separator.
                buf.push(DisplayCommand::FillRect {
                    rect: Rect::new(px + PAD_H, ry + ROW_H - 1.0, PANEL_W - PAD_H * 2.0, 1.0),
                    color: SEPARATOR,
                });
            }
        }
        _ => {
            // No certificate — single info row.
            buf.push(DisplayCommand::FillRect {
                rect: Rect::new(px, body_top, PANEL_W, ROW_H),
                color: ROW_EVEN,
            });
            buf.push(txt(
                "No certificate information (HTTP or unavailable)",
                px + PAD_H,
                body_top + ROW_H * 0.5 - 7.0,
                PANEL_W - PAD_H * 2.0,
                12.0,
                FontWeight::NORMAL,
                NO_CERT_COL,
            ));
        }
    }

    buf.push(DisplayCommand::PopClip);
}

/// Truncate a string to at most `max_chars` Unicode scalar values, appending
/// `"\u{2026}"` when cut.
fn truncate_value(s: &str, max_chars: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max_chars {
        s.to_owned()
    } else {
        chars[..max_chars].iter().collect::<String>() + "\u{2026}"
    }
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
    fn build_panel_emits_commands_with_cert() {
        let mut p = CertPanel::new();
        p.open(Some(sample_cert()));
        let mut buf = DisplayList::new();
        build_panel(&p, &mut buf, 0.0, 0.0);
        assert!(!buf.is_empty());
    }

    #[test]
    fn build_panel_emits_commands_no_cert() {
        let mut p = CertPanel::new();
        p.open(None);
        let mut buf = DisplayList::new();
        build_panel(&p, &mut buf, 0.0, 0.0);
        assert!(!buf.is_empty());
    }

    #[test]
    fn truncate_value_short() {
        assert_eq!(truncate_value("abc", 10), "abc");
    }

    #[test]
    fn truncate_value_long() {
        let s = "a".repeat(50);
        let t = truncate_value(&s, 10);
        assert!(t.ends_with('\u{2026}'));
        assert!(t.chars().count() <= 11);
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
