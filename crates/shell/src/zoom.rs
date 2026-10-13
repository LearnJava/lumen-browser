//! Per-tab page zoom logic.
//!
//! Browser zoom works by shrinking or enlarging the *CSS layout viewport*:
//! `effective_viewport = physical_viewport / zoom_factor`.
//! A zoom_factor > 1.0 means the layout uses fewer CSS px (content appears larger);
//! < 1.0 means the layout is wider than the physical window (zoomed out / smaller text).
//!
//! `<meta name=viewport initial-scale>` does NOT feed this (GAP-VVPORT срез 3):
//! per CSSOM View it sets the ratio between the layout viewport and the visual
//! viewport (`window.visualViewport`), not the layout viewport itself — see
//! `crate::relayout::meta_initial_scale` and `V8JsRuntime::meta_viewport_scale`.

/// Default page zoom — 100%.
pub const ZOOM_DEFAULT: f32 = 1.0;
/// Minimum allowed zoom — 25%.
pub const ZOOM_MIN: f32 = 0.25;
/// Maximum allowed zoom — 400%.
pub const ZOOM_MAX: f32 = 4.0;
/// Zoom step per Ctrl+= or Ctrl+- key press.
pub const ZOOM_STEP: f32 = 0.1;

/// Increase zoom by one step, clamped to [`ZOOM_MAX`].
pub fn zoom_in(current: f32) -> f32 {
    snap((current + ZOOM_STEP).min(ZOOM_MAX))
}

/// Decrease zoom by one step, clamped to [`ZOOM_MIN`].
pub fn zoom_out(current: f32) -> f32 {
    snap((current - ZOOM_STEP).max(ZOOM_MIN))
}

/// Rounds to whole percents so repeated 10 % steps do not drift (1.1000001).
fn snap(zoom: f32) -> f32 {
    (zoom * 100.0).round() / 100.0
}

/// Label for the omnibox zoom indicator: `Some(percent)` when the page is not
/// at 100 %, `None` when the indicator stays hidden.
pub fn percent_label(zoom: f32) -> Option<u32> {
    let percent = (zoom * 100.0).round();
    (percent.is_finite() && percent > 0.0 && (zoom - ZOOM_DEFAULT).abs() > 0.004)
        .then_some(percent as u32)
}

/// Lower-cased host of an `http(s)` URL — the key zoom is remembered under.
pub fn host_of(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://"))?;
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..end];
    let authority = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = authority.rsplit_once(':').map_or(authority, |(h, _)| h);
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}

/// Zoom remembered per host: `host → factor`, only hosts whose zoom differs
/// from 100 % are kept. Persisted as `host<TAB>factor` lines in
/// `<data>/zoom.txt`; a private session keeps it in memory only.
#[derive(Debug, Default)]
pub struct SiteZoom {
    map: std::collections::HashMap<String, f32>,
}

impl SiteZoom {
    /// Parses the file body; malformed or out-of-range lines are skipped.
    pub fn parse(text: &str) -> Self {
        let map = text
            .lines()
            .filter_map(|line| {
                let (host, factor) = line.split_once('\t')?;
                let factor: f32 = factor.trim().parse().ok()?;
                (!host.is_empty() && (ZOOM_MIN..=ZOOM_MAX).contains(&factor))
                    .then(|| (host.to_owned(), factor))
            })
            .collect();
        Self { map }
    }

    /// File body, sorted by host so the file is stable.
    pub fn serialize(&self) -> String {
        let mut rows: Vec<_> = self.map.iter().collect();
        rows.sort_by(|a, b| a.0.cmp(b.0));
        rows.iter().map(|(h, f)| format!("{h}\t{f}\n")).collect()
    }

    /// Remembered zoom of `host`, 100 % when none.
    pub fn get(&self, host: &str) -> f32 {
        self.map.get(host).copied().unwrap_or(ZOOM_DEFAULT)
    }

    /// Remembers `zoom` for `host`; 100 % forgets the entry.
    pub fn set(&mut self, host: &str, zoom: f32) {
        if (zoom - ZOOM_DEFAULT).abs() < 0.004 {
            self.map.remove(host);
        } else {
            self.map.insert(host.to_owned(), zoom);
        }
    }
}

fn store_path() -> Option<std::path::PathBuf> {
    let cfg = crate::config::global();
    let private =
        cfg.no_persistent_state || cfg.http_profile == lumen_network::HttpProfile::TorBrowser;
    (!private).then(|| crate::adblock::browser_data_dir().join("zoom.txt"))
}

fn global() -> &'static std::sync::Mutex<SiteZoom> {
    static STORE: std::sync::OnceLock<std::sync::Mutex<SiteZoom>> = std::sync::OnceLock::new();
    STORE.get_or_init(|| {
        let text = store_path().and_then(|p| std::fs::read_to_string(p).ok());
        std::sync::Mutex::new(text.map_or_else(SiteZoom::default, |t| SiteZoom::parse(&t)))
    })
}

/// Remembered zoom of `host` (100 % when none).
pub fn site_zoom(host: &str) -> f32 {
    global().lock().unwrap_or_else(|e| e.into_inner()).get(host)
}

/// Remembers the zoom of `host` and writes the file (best effort).
pub fn remember_site_zoom(host: &str, zoom: f32) {
    let mut g = global().lock().unwrap_or_else(|e| e.into_inner());
    g.set(host, zoom);
    if let Some(path) = store_path() {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, g.serialize());
    }
}

/// Reset zoom to 100%.
pub fn zoom_reset() -> f32 {
    ZOOM_DEFAULT
}

/// Debounce delay before a transform-first zoom step triggers a full relayout
/// (ADR-016 M0.3). Ctrl+/-/0 applies an immediate scale transform to the
/// retained display list; the expensive relayout runs once, this long after the
/// *last* zoom step, so a rapid burst of key presses reflows only once.
pub const ZOOM_RELAYOUT_DEBOUNCE_MS: u64 = 180;

/// Preview scale for transform-first zoom (ADR-016 M0.3).
///
/// The retained display list was laid out at `laid_out_zoom`; the user has since
/// moved zoom to `zoom_factor`. Until the debounced relayout runs, the backend
/// scales the existing display list by this ratio so the change is visible
/// immediately. Returns `1.0` (no preview) when either factor is non-positive or
/// non-finite.
pub fn preview_scale(zoom_factor: f32, laid_out_zoom: f32) -> f32 {
    if !zoom_factor.is_finite()
        || !laid_out_zoom.is_finite()
        || zoom_factor <= 0.0
        || laid_out_zoom <= 0.0
    {
        return 1.0;
    }
    zoom_factor / laid_out_zoom
}

/// Compute the CSS layout viewport size from the physical window size.
///
/// `zoom_factor` is the user-controlled browser zoom (Ctrl+=/Ctrl+-/Ctrl+0) —
/// the only factor that reflows the box tree; a larger factor means fewer CSS
/// px, so a smaller layout viewport.
pub fn effective_viewport(physical_width: f32, physical_height: f32, zoom_factor: f32) -> (f32, f32) {
    let scale = zoom_factor.max(f32::EPSILON);
    (physical_width / scale, physical_height / scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_in_clamps_at_max() {
        assert!((zoom_in(ZOOM_MAX)).abs() <= ZOOM_MAX + f32::EPSILON);
    }

    #[test]
    fn zoom_out_clamps_at_min() {
        assert!((zoom_out(ZOOM_MIN)) >= ZOOM_MIN - f32::EPSILON);
    }

    #[test]
    fn steps_do_not_drift() {
        let mut z = ZOOM_DEFAULT;
        for _ in 0..3 {
            z = zoom_in(z);
        }
        assert_eq!(z, 1.3);
        assert_eq!(percent_label(z), Some(130));
        assert_eq!(percent_label(ZOOM_DEFAULT), None);
    }

    #[test]
    fn host_of_url() {
        assert_eq!(host_of("https://User@Example.COM:8443/a?b#c").as_deref(), Some("example.com"));
        assert_eq!(host_of("about:blank"), None);
    }

    #[test]
    fn site_zoom_round_trip() {
        let mut z = SiteZoom::default();
        z.set("a.test", 1.5);
        z.set("b.test", 0.8);
        z.set("c.test", 1.0);
        let text = z.serialize();
        let back = SiteZoom::parse(&format!("{text}garbage\nbad.test\t9\n"));
        assert_eq!(back.get("a.test"), 1.5);
        assert_eq!(back.get("b.test"), 0.8);
        assert_eq!(back.get("c.test"), 1.0);
        assert_eq!(back.get("bad.test"), 1.0);
        z.set("a.test", 1.0);
        assert_eq!(z.get("a.test"), 1.0);
    }

    #[test]
    fn zoom_reset_returns_default() {
        assert_eq!(zoom_reset(), ZOOM_DEFAULT);
    }

    #[test]
    fn preview_scale_identity_when_unchanged() {
        assert!((preview_scale(1.0, 1.0) - 1.0).abs() < 1e-6);
        assert!((preview_scale(2.0, 2.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn preview_scale_ratio() {
        // laid out at 1.0, zoomed to 1.1 → preview by 1.1×.
        assert!((preview_scale(1.1, 1.0) - 1.1).abs() < 1e-6);
        // laid out at 2.0, zoomed back to 1.0 → shrink by half.
        assert!((preview_scale(1.0, 2.0) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn preview_scale_guards_degenerate() {
        assert_eq!(preview_scale(0.0, 1.0), 1.0);
        assert_eq!(preview_scale(1.0, 0.0), 1.0);
        assert_eq!(preview_scale(f32::NAN, 1.0), 1.0);
        assert_eq!(preview_scale(f32::INFINITY, 1.0), 1.0);
    }

    #[test]
    fn effective_viewport_no_scale() {
        let (w, h) = effective_viewport(1024.0, 768.0, 1.0);
        assert!((w - 1024.0).abs() < 0.01);
        assert!((h - 768.0).abs() < 0.01);
    }

    #[test]
    fn effective_viewport_zoom_in() {
        // zoom=2.0 → layout sees half the pixels → 512×384 CSS px
        let (w, h) = effective_viewport(1024.0, 768.0, 2.0);
        assert!((w - 512.0).abs() < 0.01);
        assert!((h - 384.0).abs() < 0.01);
    }
}
