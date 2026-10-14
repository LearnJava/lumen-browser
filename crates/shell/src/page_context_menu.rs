//! Page-level right-click context menu (P3-spell slice 3).
//!
//! Two flavours share the widget. The spell-check suggestion menu: right-clicking
//! a misspelled word in a focused text input opens a floating menu listing up to
//! five correction candidates followed by "Add to dictionary" and "Ignore".
//! The page menu (COMPAT-HANDOFF): a plain right-click on an http(s) page opens
//! «Открыть в другом браузере» with the reason choices
//! ([`HANDOFF_ROWS`]).
//!
//! Like [`crate::tabs::context_menu`], the menu is drawn by the engine chrome
//! (`#pageCtxMenu`, UX-CHROME-PANELS); this module owns only state and
//! geometry — rows are picked by the chrome hit-test and the shell performs the actual value edit /
//! dictionary mutation. Unlike the tab menu the row set is dynamic (it depends
//! on how many suggestions the checker returned), so the height and hit-testing
//! is computed from `items.len()`.

use lumen_dom::NodeId;
use lumen_chrome::ChromePageCtxModel;

use crate::external_browser::HandoffReason;

// ── Geometry (mirrors `.page-ctx` in `assets/chrome/chrome.html`) ──────────────

/// Menu width in CSS px.
pub const MENU_W: f32 = 280.0;
/// Height of one menu row in CSS px.
const ROW_H: f32 = 28.0;
/// Vertical padding above the first / below the last row.
const PAD_Y: f32 = 5.0;

/// Rows of the page menu: `(data-page-ctx value, reason, remember, label)`.
/// The first four hand the page over once, the last creates a 30-day site rule.
pub const HANDOFF_ROWS: [(&str, HandoffReason, bool, &str); 5] = [
    ("ho:other", HandoffReason::Other, false, "Открыть в другом браузере"),
    ("ho:crypto", HandoffReason::Crypto, false, "… если ГОСТ / ЭП"),
    ("ho:drm", HandoffReason::Drm, false, "… если DRM-видео"),
    ("ho:calls", HandoffReason::Calls, false, "… если видеозвонки"),
    ("ho:always", HandoffReason::Other, true, "Всегда открывать сайт там (30 дней)"),
];

// ── Types ─────────────────────────────────────────────────────────────────────

/// An action the user can pick from the page menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpellMenuAction {
    /// COMPAT-HANDOFF: hand the page to another browser (`remember` — also
    /// create the 30-day site rule).
    Handoff {
        /// Why the page is handed over — picks the browser.
        reason: HandoffReason,
        /// Create the site rule.
        remember: bool,
    },
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
    /// Ordered rows shown in the menu.
    items: Vec<SpellMenuAction>,
    /// Context for applying the chosen action.
    target: Option<SpellTarget>,
    /// `false` — строки «Добавить в словарь»/«Пропустить» отсутствуют: меню
    /// значений автозаполнения (UX-AUTOFILL срез 3), а не орфографии.
    spell_footer: bool,
    /// `true` — page menu: the «Открыть в другом браузере» rows ([`HANDOFF_ROWS`]).
    handoff: bool,
}

impl PageContextMenu {
    /// COMPAT-HANDOFF: open the page menu at cursor `(x, y)`.
    pub fn open_page(&mut self, x: f32, y: f32) {
        self.items.clear();
        self.target = None;
        self.spell_footer = false;
        self.handoff = true;
        self.anchor_x = x;
        self.anchor_y = y;
        self.open = true;
    }

    /// Open the menu at cursor `(x, y)` for `target`, offering `suggestions`
    /// (already truncated to at most five by the caller) followed by the
    /// "Add to dictionary" and "Ignore" rows.
    pub fn open_for(&mut self, x: f32, y: f32, suggestions: Vec<String>, target: SpellTarget) {
        let mut items: Vec<SpellMenuAction> =
            suggestions.into_iter().take(5).map(SpellMenuAction::Use).collect();
        items.push(SpellMenuAction::AddToDict);
        items.push(SpellMenuAction::Ignore);
        self.spell_footer = true;
        self.handoff = false;
        self.items = items;
        self.target = Some(target);
        self.anchor_x = x;
        self.anchor_y = y;
        self.open = true;
    }

    /// UX-AUTOFILL срез 3: открыть меню сохранённых значений под полем.
    /// `target.text` — введённое сейчас; выбор строки заменяет его целиком.
    pub fn open_values(&mut self, x: f32, y: f32, values: Vec<String>, target: SpellTarget) {
        self.items = values.into_iter().take(5).map(SpellMenuAction::Use).collect();
        self.spell_footer = false;
        self.handoff = false;
        self.target = Some(target);
        self.anchor_x = x;
        self.anchor_y = y;
        self.open = !self.items.is_empty();
    }

    /// Hide the menu and drop its context.
    pub fn close(&mut self) {
        self.open = false;
        self.handoff = false;
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
        let handoff = if self.handoff { HANDOFF_ROWS.len() } else { 0 };
        PAD_Y * 2.0 + ROW_H * (self.items.len() + handoff) as f32
    }

    /// Compute the clamped top-left anchor so the menu stays inside the window.
    fn anchor(&self, window_w: f32, window_h: f32) -> (f32, f32) {
        let h = self.menu_height();
        let x = self.anchor_x.min((window_w - MENU_W).max(0.0)).max(0.0);
        let y = self.anchor_y.min((window_h - h).max(0.0)).max(0.0);
        (x, y)
    }

    /// Maps a `data-page-ctx` attribute value of `#pageCtxMenu`'s rows
    /// (`use:<index>` / `add` / `ignore`) to the action it stands for.
    pub fn action_from_attr(&self, value: &str) -> Option<SpellMenuAction> {
        if self.handoff
            && let Some(&(_, reason, remember, _)) = HANDOFF_ROWS.iter().find(|r| r.0 == value)
        {
            return Some(SpellMenuAction::Handoff { reason, remember });
        }
        match value {
            "add" => Some(SpellMenuAction::AddToDict),
            "ignore" => Some(SpellMenuAction::Ignore),
            _ => {
                let idx: usize = value.strip_prefix("use:")?.parse().ok()?;
                match self.items.get(idx)? {
                    a @ SpellMenuAction::Use(_) => Some(a.clone()),
                    _ => None,
                }
            }
        }
    }

    /// Snapshot for `#pageCtxMenu`: the anchor clamped into a `window_w` ×
    /// `window_h` window and the suggestion labels.
    pub fn chrome_model(&self, window_w: f32, window_h: f32) -> ChromePageCtxModel {
        let (x, y) = self.anchor(window_w, window_h);
        ChromePageCtxModel {
            open: self.open,
            x,
            y,
            suggestions: self
                .items
                .iter()
                .filter_map(|a| match a {
                    SpellMenuAction::Use(s) => Some(s.clone()),
                    _ => None,
                })
                .collect(),
            spell_footer: self.spell_footer,
            extra_rows: if self.handoff {
                HANDOFF_ROWS.iter().map(|r| (r.0.to_owned(), r.3.to_owned())).collect()
            } else {
                Vec::new()
            },
        }
    }
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

    /// Count the visible menu rows (suggestions plus the spell footer).
    fn row_count(m: &PageContextMenu) -> usize {
        let c = m.chrome_model(1024.0, 720.0);
        c.suggestions.len() + if c.spell_footer { 2 } else { 0 } + c.extra_rows.len()
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
    fn values_menu_has_no_spell_rows() {
        let mut m = PageContextMenu::default();
        m.open_values(100.0, 50.0, vec!["Омск".to_owned(), "Орёл".to_owned()], target());
        assert!(m.is_open());
        assert_eq!(row_count(&m), 2);
        assert_eq!(m.action_from_attr("use:0"), Some(SpellMenuAction::Use("Омск".to_owned())));
        assert!(!m.chrome_model(1024.0, 720.0).spell_footer);
        m.open_values(100.0, 50.0, Vec::new(), target());
        assert!(!m.is_open());
    }

    #[test]
    fn page_menu_offers_handoff_rows() {
        let mut m = PageContextMenu::default();
        m.open_page(100.0, 50.0);
        assert!(m.is_open());
        assert!(m.target().is_none());
        assert_eq!(row_count(&m), HANDOFF_ROWS.len());
        assert_eq!(
            m.action_from_attr("ho:drm"),
            Some(SpellMenuAction::Handoff { reason: HandoffReason::Drm, remember: false })
        );
        assert_eq!(
            m.action_from_attr("ho:always"),
            Some(SpellMenuAction::Handoff { reason: HandoffReason::Other, remember: true })
        );
        assert_eq!(m.action_from_attr("add"), Some(SpellMenuAction::AddToDict));
        let c = m.chrome_model(1024.0, 720.0);
        assert!(c.open && !c.spell_footer && c.suggestions.is_empty());
    }

    #[test]
    fn spell_menu_has_no_handoff_rows() {
        let mut m = open_menu();
        assert_eq!(m.action_from_attr("ho:other"), None);
        m.open_page(0.0, 0.0);
        m.open_for(0.0, 0.0, vec!["a".to_owned()], target());
        assert!(m.chrome_model(1024.0, 720.0).extra_rows.is_empty());
    }

    #[test]
    fn default_is_closed() {
        let m = PageContextMenu::default();
        assert!(!m.is_open());
        assert!(!m.chrome_model(1024.0, 720.0).open);
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
        assert!(m.chrome_model(1024.0, 720.0).suggestions.is_empty());
        assert!(m.target().is_none());
    }

    #[test]
    fn target_apply_replaces_word() {
        let t = target();
        assert_eq!(t.apply("world"), "hello world");
    }

    #[test]
    fn chrome_model_lists_suggestions() {
        let c = open_menu().chrome_model(1024.0, 720.0);
        assert!(c.open && c.spell_footer);
        assert_eq!(c.suggestions, vec!["world".to_owned(), "would".to_owned()]);
        assert_eq!((c.x, c.y), (100.0, 50.0));
    }

    #[test]
    fn row_attrs_map_to_actions() {
        let m = open_menu();
        assert_eq!(m.action_from_attr("use:1"), Some(SpellMenuAction::Use("would".to_owned())));
        assert_eq!(m.action_from_attr("use:2"), None);
        assert_eq!(m.action_from_attr("add"), Some(SpellMenuAction::AddToDict));
        assert_eq!(m.action_from_attr("ignore"), Some(SpellMenuAction::Ignore));
        assert_eq!(m.action_from_attr("nope"), None);
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
    }
}
