//! The print dialog: opening it from a key or from the page's own
//! `window.print()`, its key handling, and rendering the confirmed job to PDF.
//!
//! The panel is `crate::panels::print_panel` and the PDF encoding lives in the
//! dump path; what is here is the sequencing - a page-initiated request has to
//! reach the same dialog a keystroke opens, and confirming it renders the
//! document that is live at that moment.

use crate::*;

impl Lumen {
    pub(crate) fn handle_print_key(&mut self, code: KeyCode, key_event: &KeyEvent) -> bool {
        if self.modifiers.control_key() || self.modifiers.super_key() {
            return false;
        }
        match code {
            KeyCode::Escape if !key_event.repeat => {
                self.print_panel.close();
                self.request_redraw();
                true
            }
            KeyCode::Backspace if self.print_panel.editing_field.is_some() => {
                self.print_panel.pop_char();
                self.request_redraw();
                true
            }
            _ => {
                if self.print_panel.editing_field.is_some()
                    && let Some(text) = key_event.text.as_ref()
                        && !text.is_empty()
                        && !text.chars().any(char::is_control)
                    {
                        for ch in text.chars() {
                            self.print_panel.push_char(ch);
                        }
                        self.request_redraw();
                        return true;
                    }
                false
            }
        }
    }

    /// Export current document as PDF using parameters from PrintRequest (W-2 Phase 3b).
    pub(crate) fn handle_print_request(&mut self, req: &lumen_js::PrintRequest) {
        // Determine output path: use provided path or generate default.
        let output_path = req
            .output_path
            .as_ref()
            .map(std::path::PathBuf::from)
            .unwrap_or_else(default_pdf_output_path);

        // Convert margins from CSS px (96 DPI) to points (1 point = 1/72 inch).
        // 1 CSS px at 96 DPI = 72/96 points = 0.75 points (not used here; we keep px).

        let margin_top = req.margin_top;
        let margin_bottom = req.margin_bottom;
        let margin_left = req.margin_left;
        let margin_right = req.margin_right;

        match do_print_to_pdf_with_opts(
            &self.source,
            &output_path,
            self.event_sink.clone(),
            PrintOptions {
                margin_tb: (margin_top + margin_bottom) / 2.0, // Simplified: average for TB and LR.
                margin_lr: (margin_left + margin_right) / 2.0,
                scale: 100, // Default scale: 100%
                print_backgrounds: true, // print background graphics (JS print request default)
                landscape: false, // BUG-420: JS `window.print()` carries no orientation — always portrait.
            },
        ) {
            Ok(page_count) => {
                eprintln!(
                    "[shell] PDF exported to {}: {} pages",
                    output_path.display(),
                    page_count
                );
                // Phase 2 future: show user feedback notification.
            }
            Err(e) => {
                eprintln!("[shell] PDF export failed: {}", e);
                // Phase 2 future: show error dialog to user.
            }
        }
    }

    /// The engine chrome's "Печать" button (`ChromeAction::PrintConfirm`,
    /// [BUG-420](../../../bugs/BUG-420-FIXED.md)) — exports the active tab
    /// with `PrintPanel`'s live settings (margin preset, scale, background
    /// graphics, orientation) and closes the dialog, mirroring
    /// `handle_print_request`'s JS `window.print()` path.
    pub(crate) fn handle_print_confirm(&mut self) {
        let output_path = default_pdf_output_path();
        let (margin_tb, margin_lr) = self.print_panel.margin_px();
        let landscape = self.print_panel.orientation == panels::print_panel::Orientation::Landscape;

        match do_print_to_pdf_with_opts(
            &self.source,
            &output_path,
            self.event_sink.clone(),
            PrintOptions {
                margin_tb,
                margin_lr,
                scale: self.print_panel.scale,
                print_backgrounds: self.print_panel.print_backgrounds,
                landscape,
            },
        ) {
            Ok(page_count) => {
                eprintln!(
                    "[shell] PDF exported to {}: {} pages",
                    output_path.display(),
                    page_count
                );
            }
            Err(e) => {
                eprintln!("[shell] PDF export failed: {}", e);
            }
        }
        self.print_panel.close();
    }
}
