//! Note viewer overlay panel (§12.2, GG-2).
//!
//! Floating overlay that shows a single annotation (selection + comment + source
//! URL) fetched from `lumen_knowledge::Notes` after the user picks an
//! `@notes`-search result from the omnibox dropdown and presses Enter.
//!
//! Layout (CSS px):
//! ```text
//!            ┌───────────────────────────────────┐
//!            │  Заметка                      [×] │ ← HEADER_H
//!            ├───────────────────────────────────┤
//!            │  🔗 https://source.url/page       │ ← URL_ROW_H
//!            ├───────────────────────────────────┤
//!            │                                   │
//!            │  "Выделенный текст со страницы"   │ ← selection
//!            │                                   │
//!            ├───────────────────────────────────┤
//!            │  Комментарий пользователя         │ ← comment (optional)
//!            └───────────────────────────────────┘
//! ```
//!
//! Keyboard: `Escape` closes the overlay (handled in shell `handle_note_viewer_key`).
//! The panel is opened via `note-viewer:<note_id>` URL scheme processed in
//! `handle_omnibox_commit`.

use lumen_core::geom::Rect;
use lumen_layout::{Color, FontStyle, FontWeight};
use lumen_paint::{DisplayCommand, DisplayList};

// ── Visual constants ──────────────────────────────────────────────────────────

/// Width of the overlay in CSS px.
pub const OVERLAY_W: f32 = 560.0;
/// Height of the panel header in CSS px.
const HEADER_H: f32 = 36.0;
/// Height of the URL row in CSS px.
const URL_ROW_H: f32 = 28.0;
/// Padding inside content areas.
const PAD: f32 = 12.0;
/// Min height of the selection area.
const SEL_MIN_H: f32 = 60.0;
/// Height of the comment row (only shown when non-empty).
const COMMENT_ROW_H: f32 = 48.0;

const BG: Color = Color { r: 24, g: 25, b: 30, a: 250 };
const HEADER_BG: Color = Color { r: 34, g: 36, b: 46, a: 255 };
const URL_BG: Color = Color { r: 18, g: 20, b: 26, a: 255 };
const SEL_BG: Color = Color { r: 28, g: 30, b: 38, a: 255 };
const COMMENT_BG: Color = Color { r: 22, g: 24, b: 32, a: 255 };
const BORDER: Color = Color { r: 55, g: 58, b: 76, a: 255 };
const ACCENT: Color = Color { r: 60, g: 120, b: 220, a: 255 };
const TEXT_HEADER: Color = Color { r: 210, g: 212, b: 228, a: 255 };
const TEXT_URL: Color = Color { r: 90, g: 150, b: 230, a: 255 };
const TEXT_SEL: Color = Color { r: 220, g: 222, b: 234, a: 255 };
const TEXT_COMMENT: Color = Color { r: 160, g: 168, b: 185, a: 255 };
const TEXT_LABEL: Color = Color { r: 100, g: 108, b: 126, a: 255 };
const CLOSE_BG: Color = Color { r: 52, g: 56, b: 74, a: 200 };
const CLOSE_FG: Color = Color { r: 148, g: 154, b: 172, a: 255 };

const FONT_HEADER: f32 = 13.0;
const FONT_URL: f32 = 11.0;
const FONT_SEL: f32 = 13.0;
const FONT_COMMENT: f32 = 12.0;
const FONT_LABEL: f32 = 10.5;

// ── Data types ────────────────────────────────────────────────────────────────

/// Which region of the overlay was hit by a mouse click.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoteHit {
    /// The [×] close button.
    Close,
    /// Any non-interactive area (header, body).
    Body,
}

/// Floating overlay for displaying a single user annotation.
///
/// Opened by `handle_omnibox_commit` when the committed value is
/// `note-viewer:<id>`. Dismissed by `Escape` or clicking [×].
pub struct NoteViewerPanel {
    /// Whether the overlay is currently visible.
    pub visible: bool,
    /// Database id of the displayed note.
    pub note_id: i64,
    /// Source URL the note was created on.
    pub url: String,
    /// The highlighted text selection from the source page.
    pub selection: String,
    /// Optional user comment attached to the note.
    pub comment: String,
}

impl NoteViewerPanel {
    /// Create a hidden panel with empty state.
    pub fn new() -> Self {
        Self {
            visible: false,
            note_id: 0,
            url: String::new(),
            selection: String::new(),
            comment: String::new(),
        }
    }

    /// Show the panel populated with the given note data.
    pub fn open(&mut self, note_id: i64, url: &str, selection: &str, comment: &str) {
        self.visible = true;
        self.note_id = note_id;
        self.url = url.to_owned();
        self.selection = selection.to_owned();
        self.comment = comment.to_owned();
    }

    /// Hide the panel (data is preserved for re-open).
    pub fn close(&mut self) {
        self.visible = false;
    }

    /// Total height of the overlay given the current content.
    pub fn panel_height(&self) -> f32 {
        let comment_h = if self.comment.is_empty() { 0.0 } else { COMMENT_ROW_H };
        HEADER_H + URL_ROW_H + SEL_MIN_H + comment_h + PAD * 2.0
    }

    /// Hit-test a click at `(px, py)` in viewport coordinates.
    ///
    /// Returns `None` if the click is outside the overlay.
    pub fn hit_test(&self, px: f32, py: f32, window_size: (u32, u32)) -> Option<NoteHit> {
        if !self.visible {
            return None;
        }
        let (ww, wh) = (window_size.0 as f32, window_size.1 as f32);
        let x = ((ww - OVERLAY_W) * 0.5).max(12.0);
        let h = self.panel_height();
        let y = ((wh - h) * 0.5).max(12.0);

        if px < x || px > x + OVERLAY_W || py < y || py > y + h {
            return None;
        }

        // Close button: top-right corner of header.
        let close_x = x + OVERLAY_W - PAD - 18.0;
        let close_y = y + (HEADER_H - 18.0) * 0.5;
        if px >= close_x && px <= close_x + 18.0 && py >= close_y && py <= close_y + 18.0 {
            return Some(NoteHit::Close);
        }

        Some(NoteHit::Body)
    }
}

impl Default for NoteViewerPanel {
    fn default() -> Self {
        Self::new()
    }
}

// ── Renderer ──────────────────────────────────────────────────────────────────

/// Build the display list for the note viewer overlay.
///
/// Returns an empty list if `panel.visible` is false.
pub fn build_note_viewer(panel: &NoteViewerPanel, window_size: (u32, u32)) -> DisplayList {
    if !panel.visible {
        return DisplayList::new();
    }

    let (ww, wh) = (window_size.0 as f32, window_size.1 as f32);
    let h = panel.panel_height();
    let x = ((ww - OVERLAY_W) * 0.5).max(12.0);
    let y = ((wh - h) * 0.5).max(12.0);

    let mut out = DisplayList::with_capacity(24);

    // Dim backdrop.
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(0.0, 0.0, ww, wh),
        color: Color { r: 0, g: 0, b: 0, a: 140 },
    });

    // Outer border.
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(x - 1.0, y - 1.0, OVERLAY_W + 2.0, h + 2.0),
        color: ACCENT,
    });
    // Main background.
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(x, y, OVERLAY_W, h),
        color: BG,
    });

    // ── Header ────────────────────────────────────────────────────────────────

    out.push(DisplayCommand::FillRect {
        rect: Rect::new(x, y, OVERLAY_W, HEADER_H),
        color: HEADER_BG,
    });
    out.push(DisplayCommand::DrawText {
        rect: Rect::new(x + PAD, y + (HEADER_H - FONT_HEADER * 1.3) * 0.5, OVERLAY_W - 60.0, FONT_HEADER * 1.3),
        text: "Заметка".to_string(),
        font_size: FONT_HEADER,
        color: TEXT_HEADER,
        font_family: Vec::new(),
        font_weight: FontWeight::BOLD,
        font_style: FontStyle::Normal,
        font_variation_axes: Vec::new(),
        tab_size: 0.0,
        highlight_name: None,
    });

    // Close button [×].
    let close_x = x + OVERLAY_W - PAD - 18.0;
    let close_y = y + (HEADER_H - 18.0) * 0.5;
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(close_x, close_y, 18.0, 18.0),
        color: CLOSE_BG,
    });
    out.push(DisplayCommand::DrawText {
        rect: Rect::new(close_x, close_y, 18.0, 18.0),
        text: "×".to_string(),
        font_size: 14.0,
        color: CLOSE_FG,
        font_family: Vec::new(),
        font_weight: FontWeight::NORMAL,
        font_style: FontStyle::Normal,
        font_variation_axes: Vec::new(),
        tab_size: 0.0,
        highlight_name: None,
    });

    // ── URL row ───────────────────────────────────────────────────────────────

    let url_y = y + HEADER_H;
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(x, url_y, OVERLAY_W, URL_ROW_H),
        color: URL_BG,
    });
    out.push(DisplayCommand::DrawText {
        rect: Rect::new(x + PAD, url_y + (URL_ROW_H - FONT_URL * 1.3) * 0.5, OVERLAY_W - PAD * 2.0, FONT_URL * 1.3),
        text: panel.url.clone(),
        font_size: FONT_URL,
        color: TEXT_URL,
        font_family: Vec::new(),
        font_weight: FontWeight::NORMAL,
        font_style: FontStyle::Normal,
        font_variation_axes: Vec::new(),
        tab_size: 0.0,
        highlight_name: None,
    });

    // ── Selection area ────────────────────────────────────────────────────────

    let sel_y = url_y + URL_ROW_H;
    let sel_h = SEL_MIN_H + PAD * 2.0;

    // Separator.
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(x, sel_y, OVERLAY_W, 1.0),
        color: BORDER,
    });
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(x, sel_y + 1.0, OVERLAY_W, sel_h - 1.0),
        color: SEL_BG,
    });

    // Left accent bar.
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(x + PAD, sel_y + PAD, 3.0, sel_h - PAD * 2.0),
        color: ACCENT,
    });

    out.push(DisplayCommand::DrawText {
        rect: Rect::new(x + PAD + 10.0, sel_y + PAD, OVERLAY_W - PAD * 2.0 - 10.0, sel_h - PAD * 2.0),
        text: panel.selection.clone(),
        font_size: FONT_SEL,
        color: TEXT_SEL,
        font_family: Vec::new(),
        font_weight: FontWeight::NORMAL,
        font_style: FontStyle::Italic,
        font_variation_axes: Vec::new(),
        tab_size: 0.0,
        highlight_name: None,
    });

    // ── Comment area (only if non-empty) ──────────────────────────────────────

    if !panel.comment.is_empty() {
        let cmt_y = sel_y + sel_h;
        out.push(DisplayCommand::FillRect {
            rect: Rect::new(x, cmt_y, OVERLAY_W, 1.0),
            color: BORDER,
        });
        out.push(DisplayCommand::FillRect {
            rect: Rect::new(x, cmt_y + 1.0, OVERLAY_W, COMMENT_ROW_H - 1.0),
            color: COMMENT_BG,
        });
        out.push(DisplayCommand::DrawText {
            rect: Rect::new(x + PAD, cmt_y + 4.0, 60.0, FONT_LABEL * 1.3),
            text: "Комментарий:".to_string(),
            font_size: FONT_LABEL,
            color: TEXT_LABEL,
            font_family: Vec::new(),
            font_weight: FontWeight::NORMAL,
            font_style: FontStyle::Normal,
            font_variation_axes: Vec::new(),
            tab_size: 0.0,
            highlight_name: None,
        });
        out.push(DisplayCommand::DrawText {
            rect: Rect::new(x + PAD, cmt_y + 4.0 + FONT_LABEL * 1.5, OVERLAY_W - PAD * 2.0, FONT_COMMENT * 1.3),
            text: panel.comment.clone(),
            font_size: FONT_COMMENT,
            color: TEXT_COMMENT,
            font_family: Vec::new(),
            font_weight: FontWeight::NORMAL,
            font_style: FontStyle::Normal,
            font_variation_axes: Vec::new(),
            tab_size: 0.0,
            highlight_name: None,
        });
    }

    out
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_panel() -> NoteViewerPanel {
        let mut p = NoteViewerPanel::new();
        p.open(42, "https://example.com/page", "important insight", "my comment");
        p
    }

    #[test]
    fn new_is_hidden() {
        let p = NoteViewerPanel::new();
        assert!(!p.visible);
    }

    #[test]
    fn open_sets_fields_and_visible() {
        let p = make_panel();
        assert!(p.visible);
        assert_eq!(p.note_id, 42);
        assert_eq!(p.url, "https://example.com/page");
        assert_eq!(p.selection, "important insight");
        assert_eq!(p.comment, "my comment");
    }

    #[test]
    fn close_hides_panel() {
        let mut p = make_panel();
        p.close();
        assert!(!p.visible);
        // Data preserved.
        assert_eq!(p.selection, "important insight");
    }

    #[test]
    fn panel_height_grows_with_comment() {
        let mut p = NoteViewerPanel::new();
        p.open(1, "https://x/", "sel", "");
        let h_no_comment = p.panel_height();
        p.open(1, "https://x/", "sel", "has comment");
        let h_with_comment = p.panel_height();
        assert!(h_with_comment > h_no_comment);
    }

    #[test]
    fn hit_test_returns_none_when_hidden() {
        let p = NoteViewerPanel::new(); // not open
        assert_eq!(p.hit_test(512.0, 360.0, (1024, 720)), None);
    }

    #[test]
    fn hit_test_close_button() {
        let p = make_panel();
        let (ww, wh) = (1024u32, 720u32);
        let h = p.panel_height();
        let x = ((ww as f32 - OVERLAY_W) * 0.5).max(12.0);
        let y = ((wh as f32 - h) * 0.5).max(12.0);
        // Click in the close button region.
        let close_x = x + OVERLAY_W - PAD - 18.0 + 5.0;
        let close_y = y + (HEADER_H - 18.0) * 0.5 + 5.0;
        assert_eq!(p.hit_test(close_x, close_y, (ww, wh)), Some(NoteHit::Close));
    }

    #[test]
    fn hit_test_body_inside_overlay() {
        let p = make_panel();
        let (ww, wh) = (1024u32, 720u32);
        let h = p.panel_height();
        let x = ((ww as f32 - OVERLAY_W) * 0.5).max(12.0);
        let y = ((wh as f32 - h) * 0.5).max(12.0);
        // Click in the selection area.
        let body_x = x + 100.0;
        let body_y = y + HEADER_H + URL_ROW_H + 20.0;
        assert_eq!(p.hit_test(body_x, body_y, (ww, wh)), Some(NoteHit::Body));
    }

    #[test]
    fn build_overlay_empty_when_hidden() {
        let p = NoteViewerPanel::new();
        let dl = build_note_viewer(&p, (1024, 720));
        assert!(dl.is_empty());
    }

    #[test]
    fn build_overlay_has_header_and_selection_text() {
        let p = make_panel();
        let dl = build_note_viewer(&p, (1024, 720));
        let texts: Vec<&str> = dl.iter().filter_map(|c| {
            if let DisplayCommand::DrawText { text, .. } = c { Some(text.as_str()) } else { None }
        }).collect();
        assert!(texts.iter().any(|t| t.contains("Заметка")));
        assert!(texts.iter().any(|t| t.contains("important insight")));
        assert!(texts.iter().any(|t| t.contains("example.com")));
    }
}
