//! Page-level right-click context menu (P3-spell slice 3).
//!
//! Currently this is the spell-check suggestion menu: right-clicking a
//! misspelled word in a focused text input opens a floating menu listing up to
//! five correction candidates followed by "Add to dictionary" and "Ignore".
//!
//! Like [`crate::tabs::context_menu`], this module owns only geometry,
//! rendering and hit-testing — the shell performs the actual value edit /
//! dictionary mutation. Unlike the tab menu the row set is dynamic (it depends
//! on how many suggestions the checker returned), so the height and hit-testing
//! are computed from `items.len()`.

use lumen_core::geom::Rect;
use lumen_dom::NodeId;
use lumen_layout::{Color, FontStyle, FontWeight};
use lumen_paint::{CornerRadii, DisplayCommand, DisplayList};

// ── Visual constants (mirror the dark-chrome tab context menu) ────────────────

/// Menu width in CSS px.
pub const MENU_W: f32 = 230.0;
/// Height of one menu row in CSS px.
const ROW_H: f32 = 28.0;
/// Vertical padding above the first / below the last row.
const PAD_Y: f32 = 5.0;
/// Left text inset inside a row.
const TEXT_PAD_X: f32 = 14.0;
/// Row text font size.
const FONT_SZ: f32 = 13.0;
/// Corner radius of the menu background.
const RADIUS: f32 = 6.0;

const MENU_BG: Color = Color { r: 38, g: 39, b: 44, a: 250 };
const MENU_BORDER: Color = Color { r: 70, g: 71, b: 78, a: 255 };
const ROW_HOVER_BG: Color = Color { r: 58, g: 78, b: 120, a: 255 };
const ITEM_TEXT: Color = Color { r: 222, g: 222, b: 230, a: 255 };
/// Suggestion rows are drawn slightly brighter to read as the primary action.
const SUGGEST_TEXT: Color = Color { r: 236, g: 236, b: 244, a: 255 };
const DIVIDER: Color = Color { r: 60, g: 61, b: 68, a: 255 };

// ── Types ─────────────────────────────────────────────────────────────────────

/// An action the user can pick from the spell suggestion menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpellMenuAction {
    /// Replace the misspelled word with this suggestion.
    Use(String),
    /// Add the misspelled word to the persistent user dictionary.
    AddToDict,
    /// Ignore the misspelled word for the rest of the session.
    Ignore,
}

/// How a [`SpellTarget`] correction gets written back to the DOM (P3-spell
/// slice 4) — the three control kinds [`crate::spell_target`] recognizes
/// store their editable text differently, so applying a suggestion needs to
/// know which one it's dealing with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpellTargetKind {
    /// Single-line `<input>` — value lives in the `value` attribute.
    Input,
    /// `<textarea>` — value lives in direct text-node children.
    Textarea,
    /// A `contenteditable` host — content is arbitrary rich-text DOM, so only
    /// the misspelled word's own text node is spliced in place.
    ContentEditable,
}

/// Everything the shell needs to apply the chosen action: which control holds
/// the word, the word's byte range inside that control's rendered text, and the
/// word text itself.
#[derive(Debug, Clone)]
pub struct SpellTarget {
    /// The focused control whose text is edited: an `<input>`/`<textarea>`
    /// node, or the `contenteditable` editing host.
    pub node: NodeId,
    /// Full logical text of the control — its `value` for `<input>`, its
    /// full (possibly multi-line) value for `<textarea>`, or the
    /// concatenated `textContent` for a contenteditable host. Used to rebuild
    /// the value / locate the word's text node when applying a suggestion.
    pub text: String,
    /// Start byte offset of the misspelled word inside `text`.
    pub word_start: usize,
    /// End byte offset (exclusive) of the misspelled word inside `text`.
    pub word_end: usize,
    /// Which kind of control `node` is — determines how `Use` writes back.
    pub kind: SpellTargetKind,
}

impl SpellTarget {
    /// The misspelled word slice.
    pub fn word(&self) -> &str {
        &self.text[self.word_start..self.word_end]
    }

    /// Rebuild the control's value with the word replaced by `replacement`.
    pub fn apply(&self, replacement: &str) -> String {
        let mut out = String::with_capacity(self.text.len() + replacement.len());
        out.push_str(&self.text[..self.word_start]);
        out.push_str(replacement);
        out.push_str(&self.text[self.word_end..]);
        out
    }
}

/// State of the page-level spell suggestion menu. One menu is open at a time.
#[derive(Debug, Default)]
pub struct PageContextMenu {
    /// Whether the menu is currently visible.
    open: bool,
    /// Raw cursor X where the menu was summoned, CSS px (pre-clamp).
    anchor_x: f32,
    /// Raw cursor Y where the menu was summoned, CSS px (pre-clamp).
    anchor_y: f32,
    /// Row index currently under the cursor, for hover highlight.
    pub hovered: Option<usize>,
    /// Ordered rows shown in the menu.
    items: Vec<SpellMenuAction>,
    /// Context for applying the chosen action.
    target: Option<SpellTarget>,
}

impl PageContextMenu {
    /// Open the menu at cursor `(x, y)` for `target`, offering `suggestions`
    /// (already truncated to at most five by the caller) followed by the
    /// "Add to dictionary" and "Ignore" rows.
    pub fn open_for(&mut self, x: f32, y: f32, suggestions: Vec<String>, target: SpellTarget) {
        let mut items: Vec<SpellMenuAction> =
            suggestions.into_iter().take(5).map(SpellMenuAction::Use).collect();
        items.push(SpellMenuAction::AddToDict);
        items.push(SpellMenuAction::Ignore);
        self.items = items;
        self.target = Some(target);
        self.anchor_x = x;
        self.anchor_y = y;
        self.hovered = None;
        self.open = true;
    }

    /// Hide the menu and drop its context.
    pub fn close(&mut self) {
        self.open = false;
        self.hovered = None;
        self.items.clear();
        self.target = None;
    }

    /// `true` while the menu is visible.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// The target context (word + control), if the menu is open.
    pub fn target(&self) -> Option<&SpellTarget> {
        self.target.as_ref()
    }

    /// Total menu height in CSS px (background box).
    fn menu_height(&self) -> f32 {
        PAD_Y * 2.0 + ROW_H * self.items.len() as f32
    }

    /// Index of the first "Add to dictionary" / "Ignore" row (the divider sits
    /// above it). Equals the suggestion count.
    fn divider_row(&self) -> usize {
        self.items.len().saturating_sub(2)
    }

    /// Compute the clamped top-left anchor so the menu stays inside the window.
    fn anchor(&self, window_w: f32, window_h: f32) -> (f32, f32) {
        let h = self.menu_height();
        let x = self.anchor_x.min((window_w - MENU_W).max(0.0)).max(0.0);
        let y = self.anchor_y.min((window_h - h).max(0.0)).max(0.0);
        (x, y)
    }

    /// Map a CSS-px `(x, y)` to the row index under it, or `None`.
    pub fn item_at(&self, x: f32, y: f32, window_w: f32, window_h: f32) -> Option<usize> {
        if !self.open {
            return None;
        }
        let (x0, y0) = self.anchor(window_w, window_h);
        let h = self.menu_height();
        if x < x0 || x >= x0 + MENU_W || y < y0 || y >= y0 + h {
            return None;
        }
        let row_top = y0 + PAD_Y;
        if y < row_top {
            return None;
        }
        let idx = ((y - row_top) / ROW_H) as usize;
        if idx < self.items.len() { Some(idx) } else { None }
    }

    /// Map a CSS-px `(x, y)` to the [`SpellMenuAction`] under it, or `None`.
    pub fn action_at(&self, x: f32, y: f32, window_w: f32, window_h: f32) -> Option<SpellMenuAction> {
        self.item_at(x, y, window_w, window_h).map(|i| self.items[i].clone())
    }

    /// Build a viewport-locked display list for the open menu; empty when closed.
    pub fn build_overlay(&self, window_w: f32, window_h: f32) -> DisplayList {
        if !self.open {
            return DisplayList::new();
        }
        let (x0, y0) = self.anchor(window_w, window_h);
        let h = self.menu_height();
        let mut out = DisplayList::with_capacity(self.items.len() * 2 + 2);

        // Border (drawn 1 px larger behind the background fill).
        out.push(DisplayCommand::FillRoundedRect {
            rect: Rect::new(x0 - 1.0, y0 - 1.0, MENU_W + 2.0, h + 2.0),
            radii: corners(RADIUS),
            color: MENU_BORDER,
        });
        // Background.
        out.push(DisplayCommand::FillRoundedRect {
            rect: Rect::new(x0, y0, MENU_W, h),
            radii: corners(RADIUS),
            color: MENU_BG,
        });

        let divider_row = self.divider_row();
        for (i, action) in self.items.iter().enumerate() {
            let row_y = y0 + PAD_Y + i as f32 * ROW_H;

            // Hover highlight.
            if self.hovered == Some(i) {
                out.push(DisplayCommand::FillRect {
                    rect: Rect::new(x0 + 2.0, row_y, MENU_W - 4.0, ROW_H),
                    color: ROW_HOVER_BG,
                });
            }

            // Divider above the first non-suggestion row (skip when there are
            // no suggestions — the divider would sit at the very top).
            if i == divider_row && divider_row > 0 {
                out.push(DisplayCommand::FillRect {
                    rect: Rect::new(x0 + 8.0, row_y - 1.0, MENU_W - 16.0, 1.0),
                    color: DIVIDER,
                });
            }

            let (text, color) = match action {
                SpellMenuAction::Use(s) => (s.clone(), SUGGEST_TEXT),
                SpellMenuAction::AddToDict => ("Добавить в словарь".to_owned(), ITEM_TEXT),
                SpellMenuAction::Ignore => ("Пропустить".to_owned(), ITEM_TEXT),
            };

            out.push(DisplayCommand::DrawText {
                rect: Rect::new(
                    x0 + TEXT_PAD_X,
                    row_y + (ROW_H - FONT_SZ * 1.3) * 0.5,
                    MENU_W - TEXT_PAD_X * 2.0,
                    FONT_SZ * 1.3,
                ),
                text,
                font_size: FONT_SZ,
                color,
                font_family: Vec::new(),
                font_weight: FontWeight::NORMAL,
                font_style: FontStyle::Normal,
                font_variation_axes: Vec::new(),
                font_features: Vec::new(),
                font_palette: None,
                tab_size: 0.0,
                highlight_name: None,
                text_orientation: None,
            });
        }

        out
    }
}

/// Uniform corner radii helper.
fn corners(r: f32) -> CornerRadii {
    CornerRadii { tl: r, tl_y: r, tr: r, tr_y: r, br: r, br_y: r, bl: r, bl_y: r }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn target() -> SpellTarget {
        // "hello wrold" — the second word is misspelled.
        SpellTarget {
            node: NodeId::from_index(7),
            text: "hello wrold".to_owned(),
            word_start: 6,
            word_end: 11,
            kind: SpellTargetKind::Input,
        }
    }

    /// Count the visible menu rows via the rendered overlay.
    fn row_count(m: &PageContextMenu) -> usize {
        m.build_overlay(1024.0, 720.0)
            .iter()
            .filter(|c| matches!(c, DisplayCommand::DrawText { .. }))
            .count()
    }

    fn open_menu() -> PageContextMenu {
        let mut m = PageContextMenu::default();
        m.open_for(
            100.0,
            50.0,
            vec!["world".to_owned(), "would".to_owned()],
            target(),
        );
        m
    }

    #[test]
    fn default_is_closed() {
        let m = PageContextMenu::default();
        assert!(!m.is_open());
        assert!(m.build_overlay(1024.0, 720.0).is_empty());
        assert_eq!(m.item_at(100.0, 50.0, 1024.0, 720.0), None);
    }

    #[test]
    fn open_builds_rows_and_target() {
        let m = open_menu();
        assert!(m.is_open());
        // 2 suggestions + AddToDict + Ignore.
        assert_eq!(row_count(&m), 4);
        assert_eq!(m.target().unwrap().word(), "wrold");
    }

    #[test]
    fn suggestions_capped_at_five() {
        let mut m = PageContextMenu::default();
        let many: Vec<String> = (0..9).map(|i| format!("s{i}")).collect();
        m.open_for(0.0, 0.0, many, target());
        // 5 suggestions + AddToDict + Ignore.
        assert_eq!(row_count(&m), 7);
    }

    #[test]
    fn close_clears_state() {
        let mut m = open_menu();
        m.close();
        assert!(!m.is_open());
        assert_eq!(row_count(&m), 0);
        assert!(m.target().is_none());
    }

    #[test]
    fn target_apply_replaces_word() {
        let t = target();
        assert_eq!(t.apply("world"), "hello world");
    }

    #[test]
    fn overlay_emits_all_text_rows() {
        let m = open_menu();
        let dl = m.build_overlay(1024.0, 720.0);
        let rows = dl
            .iter()
            .filter(|c| matches!(c, DisplayCommand::DrawText { .. }))
            .count();
        assert_eq!(rows, 4);
        let has = |needle: &str| {
            dl.iter().any(|c| matches!(c, DisplayCommand::DrawText { text, .. } if text == needle))
        };
        assert!(has("world"));
        assert!(has("Добавить в словарь"));
        assert!(has("Пропустить"));
    }

    #[test]
    fn action_at_maps_rows() {
        let m = open_menu();
        let y0 = 50.0 + PAD_Y;
        assert_eq!(
            m.action_at(110.0, y0 + 2.0, 1024.0, 720.0),
            Some(SpellMenuAction::Use("world".to_owned()))
        );
        assert_eq!(
            m.action_at(110.0, y0 + ROW_H + 2.0, 1024.0, 720.0),
            Some(SpellMenuAction::Use("would".to_owned()))
        );
        assert_eq!(
            m.action_at(110.0, y0 + 2.0 * ROW_H + 2.0, 1024.0, 720.0),
            Some(SpellMenuAction::AddToDict)
        );
        assert_eq!(
            m.action_at(110.0, y0 + 3.0 * ROW_H + 2.0, 1024.0, 720.0),
            Some(SpellMenuAction::Ignore)
        );
    }

    #[test]
    fn action_at_outside_is_none() {
        let m = open_menu();
        assert_eq!(m.action_at(10.0, 60.0, 1024.0, 720.0), None);
        assert_eq!(m.action_at(110.0, 50.0 + m.menu_height() + 20.0, 1024.0, 720.0), None);
    }

    #[test]
    fn menu_clamps_to_window_edges() {
        let mut m = PageContextMenu::default();
        m.open_for(1000.0, 715.0, vec!["a".to_owned()], target());
        let (x0, y0) = m.anchor(1024.0, 720.0);
        assert!(x0 + MENU_W <= 1024.0);
        assert!(y0 + m.menu_height() <= 720.0);
    }

    #[test]
    fn no_suggestions_still_has_dict_and_ignore() {
        let mut m = PageContextMenu::default();
        m.open_for(10.0, 10.0, Vec::new(), target());
        assert_eq!(row_count(&m), 2);
        let y0 = 10.0 + PAD_Y;
        assert_eq!(m.action_at(20.0, y0 + 2.0, 1024.0, 720.0), Some(SpellMenuAction::AddToDict));
        assert_eq!(
            m.action_at(20.0, y0 + ROW_H + 2.0, 1024.0, 720.0),
            Some(SpellMenuAction::Ignore)
        );
    }
}
