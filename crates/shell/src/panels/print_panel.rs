//! Print dialog panel (E-1 + W-2b) — state only.
//!
//! Opened by `Ctrl+P`; under the engine chrome the dialog itself is
//! `#printOverlay` (`assets/chrome/chrome.html`), which owns its geometry and
//! controls. The legacy display-list renderer was removed in CC-15-4 and the
//! legacy hit-test in CC-15-6.
//!
//! The settings below (paper size, orientation, margins, scale, page range,
//! colour mode, backgrounds, output path) are **not** wired to the engine
//! dialog yet — see BUG-420.

// ── Domain types ──────────────────────────────────────────────────────────────

/// Paper size for the print job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaperSize {
    /// ISO A4 (210 × 297 mm).
    A4,
    /// US Letter (8.5 × 11 in).
    #[allow(dead_code, reason = "BUG-420: настройки печати ещё не перенесены в движковый #printOverlay")]
    Letter,
    /// US Legal (8.5 × 14 in).
    #[allow(dead_code, reason = "BUG-420: настройки печати ещё не перенесены в движковый #printOverlay")]
    Legal,
}

/// Page orientation for the print job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    /// Taller than wide.
    Portrait,
    /// Wider than tall.
    #[allow(dead_code, reason = "BUG-420: настройки печати ещё не перенесены в движковый #printOverlay")]
    Landscape,
}

/// Margin preset for the print job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarginPreset {
    /// Standard ~19 mm margins.
    Normal,
    /// Small ~6 mm margins.
    #[allow(dead_code, reason = "BUG-420: настройки печати ещё не перенесены в движковый #printOverlay")]
    Narrow,
    /// Large ~25 mm margins.
    #[allow(dead_code, reason = "BUG-420: настройки печати ещё не перенесены в движковый #printOverlay")]
    Wide,
}

/// Output colour mode for the print job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorMode {
    /// Full-colour output.
    Color,
    /// Greyscale output.
    #[allow(dead_code, reason = "BUG-420: настройки печати ещё не перенесены в движковый #printOverlay")]
    Grayscale,
}

/// Which editable text field currently has keyboard focus in the print panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrintField {
    /// The page-range text field (e.g. `"all"` or `"1-3,5"`).
    #[allow(dead_code, reason = "BUG-420: настройки печати ещё не перенесены в движковый #printOverlay")]
    PageRange,
    /// The output-file path text field (e.g. `"output.pdf"`).
    #[allow(dead_code, reason = "BUG-420: настройки печати ещё не перенесены в движковый #printOverlay")]
    OutputPath,
}

// ── Panel state ───────────────────────────────────────────────────────────────

/// Print dialog panel state.
///
/// `visible` gates rendering and hit-testing. The fields are working
/// copies of the print settings; they are read in the shell handler
/// when the user clicks **Print** to kick off `do_print_to_pdf()`.
pub struct PrintPanel {
    /// Whether the panel is currently visible.
    pub visible: bool,
    /// Selected paper size.
    #[allow(dead_code, reason = "BUG-420: настройки печати ещё не перенесены в движковый #printOverlay")]
    pub paper: PaperSize,
    /// Selected page orientation.
    #[allow(dead_code, reason = "BUG-420: настройки печати ещё не перенесены в движковый #printOverlay")]
    pub orientation: Orientation,
    /// Selected margin preset.
    #[allow(dead_code, reason = "BUG-420: настройки печати ещё не перенесены в движковый #printOverlay")]
    pub margins: MarginPreset,
    /// Document zoom level in percent (50–200%, W-2b new field).
    #[allow(dead_code, reason = "BUG-420: настройки печати ещё не перенесены в движковый #printOverlay")]
    pub scale: i32,
    /// Page range string: `"all"` or an explicit range such as `"1-3,5"`.
    pub page_range: String,
    /// Output colour mode.
    #[allow(dead_code, reason = "BUG-420: настройки печати ещё не перенесены в движковый #printOverlay")]
    pub color_mode: ColorMode,
    /// Whether CSS background graphics are printed (CC-8). When `false`, the
    /// print pipeline strips background fills / images / gradients before
    /// rasterising each page.
    #[allow(dead_code, reason = "BUG-420: настройки печати ещё не перенесены в движковый #printOverlay")]
    pub print_backgrounds: bool,
    /// Destination file path (relative or absolute).
    pub output_path: String,
    /// Which text field is currently focused, if any.
    pub editing_field: Option<PrintField>,
}

impl PrintPanel {
    /// Create a new hidden panel with default print settings.
    pub fn new() -> Self {
        Self {
            visible: false,
            paper: PaperSize::A4,
            orientation: Orientation::Portrait,
            margins: MarginPreset::Normal,
            scale: 100,
            page_range: "all".to_owned(),
            color_mode: ColorMode::Color,
            print_backgrounds: true,
            output_path: "output.pdf".to_owned(),
            editing_field: None,
        }
    }

    /// Toggle panel visibility; clears the active editing field on hide.
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
        if !self.visible {
            self.editing_field = None;
        }
    }

    /// Hide the panel and clear the editing field.
    pub fn close(&mut self) {
        self.visible = false;
        self.editing_field = None;
    }

    /// Append a character to the currently focused text field.
    pub fn push_char(&mut self, ch: char) {
        match self.editing_field {
            Some(PrintField::PageRange) => self.page_range.push(ch),
            Some(PrintField::OutputPath) => self.output_path.push(ch),
            None => {}
        }
    }

    /// Delete the last character from the currently focused text field.
    pub fn pop_char(&mut self) {
        match self.editing_field {
            Some(PrintField::PageRange) => { self.page_range.pop(); }
            Some(PrintField::OutputPath) => { self.output_path.pop(); }
            None => {}
        }
    }

    /// Resolve margin values (top/bottom, left/right) in CSS px at 96 DPI.
    ///
    /// Used by the shell to build [`lumen_layout::PaginationContext`].
    #[allow(dead_code, reason = "BUG-420: настройки печати ещё не перенесены в движковый #printOverlay")]
    pub fn margin_px(&self) -> (f32, f32) {
        match self.margins {
            MarginPreset::Normal => (48.0, 48.0),
            MarginPreset::Narrow => (18.0, 18.0),
            MarginPreset::Wide => (72.0, 72.0),
        }
    }
}

impl Default for PrintPanel {
    fn default() -> Self {
        Self::new()
    }
}

// ── Hit testing ───────────────────────────────────────────────────────────────

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_panel() -> PrintPanel {
        PrintPanel::new()
    }

    // ── State ─────────────────────────────────────────────────────────────────

    #[test]
    fn new_panel_hidden_with_defaults() {
        let p = make_panel();
        assert!(!p.visible);
        assert_eq!(p.paper, PaperSize::A4);
        assert_eq!(p.orientation, Orientation::Portrait);
        assert_eq!(p.margins, MarginPreset::Normal);
        assert_eq!(p.scale, 100);
        assert_eq!(p.page_range, "all");
        assert_eq!(p.color_mode, ColorMode::Color);
        assert_eq!(p.output_path, "output.pdf");
        assert_eq!(p.editing_field, None);
    }

    #[test]
    fn toggle_shows_and_hides() {
        let mut p = make_panel();
        p.toggle();
        assert!(p.visible);
        p.toggle();
        assert!(!p.visible);
    }

    #[test]
    fn toggle_clears_editing_field_on_hide() {
        let mut p = make_panel();
        p.visible = true;
        p.editing_field = Some(PrintField::PageRange);
        p.toggle();
        assert!(!p.visible);
        assert_eq!(p.editing_field, None);
    }

    #[test]
    fn push_pop_char_in_page_range() {
        let mut p = make_panel();
        p.visible = true;
        p.editing_field = Some(PrintField::PageRange);
        p.page_range.clear();
        p.push_char('1');
        p.push_char('-');
        p.push_char('3');
        assert_eq!(p.page_range, "1-3");
        p.pop_char();
        assert_eq!(p.page_range, "1-");
    }

    #[test]
    fn push_pop_char_in_output_path() {
        let mut p = make_panel();
        p.editing_field = Some(PrintField::OutputPath);
        p.output_path = "doc".to_owned();
        p.push_char('.');
        p.push_char('p');
        assert_eq!(p.output_path, "doc.p");
        p.pop_char();
        assert_eq!(p.output_path, "doc.");
    }

    #[test]
    fn margin_px_values() {
        let mut p = make_panel();
        assert_eq!(p.margin_px(), (48.0, 48.0));
        p.margins = MarginPreset::Narrow;
        assert_eq!(p.margin_px(), (18.0, 18.0));
        p.margins = MarginPreset::Wide;
        assert_eq!(p.margin_px(), (72.0, 72.0));
    }

    #[test]
    fn scale_default_100_percent() {
        let p = make_panel();
        assert_eq!(p.scale, 100);
    }

    #[test]
    fn print_backgrounds_default_on() {
        let p = make_panel();
        assert!(p.print_backgrounds);
    }

    #[test]
    fn scale_can_increase_decrease() {
        let mut p = make_panel();
        p.scale = 100;
        p.scale = (p.scale + 10).min(200);
        assert_eq!(p.scale, 110);
        p.scale = (p.scale - 10).max(50);
        assert_eq!(p.scale, 100);
    }
}
