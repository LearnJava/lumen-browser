//! Command palette (7-series shell UI, task #23 / §7E.2).
//!
//! A modal overlay — toggled with `Ctrl+K` — that lets the user fuzzy-search
//! across three kinds of targets and act on them with the keyboard only:
//!
//! * **Commands** — a curated set of browser actions ([`PaletteAction`]) such as
//!   *New Tab*, *Reload*, *Find on Page*.
//! * **Bookmarks** — every saved bookmark (title + URL), opened on activation.
//! * **History** — recently visited pages (title + URL), opened on activation.
//!
//! Layout (centred over a dimmed full-window scrim):
//!
//! ```text
//!            ┌───────────────────────────────────────────────┐
//!            │ > new tab                                      │  input row
//!            ├───────────────────────────────────────────────┤
//!            │ ⚡ New Tab                          Ctrl+T     │  ← selected
//!            │ ★ Rust — programming language    rust-lang.org │
//!            │ ◷ Example Domain                  example.com  │
//!            │ …                                              │
//!            └───────────────────────────────────────────────┘
//! ```
//!
//! The palette is **modal**: while visible it captures every key and pointer
//! event (the scrim swallows clicks outside the box, closing the palette).
//! State lives on `Lumen`; the shell drives it via [`CommandPalette::set_items`]
//! (on open), [`hit_test`] (clicks) and [`build_panel`] (rendering), mirroring
//! the ad-hoc panel convention used by [`super::bookmark_panel`].
//!
//! Fuzzy matching ([`fuzzy_score`]) is a subsequence matcher with bonuses for
//! consecutive runs and word-boundary starts, so `nt` ranks *New Tab* above
//! *Print*. An empty query shows every item in insertion order (commands first).

use lumen_core::geom::Rect;
use lumen_layout::{Color, FontStyle, FontWeight};
use lumen_paint::{CornerRadii, DisplayCommand, DisplayList};

// ── Visual constants ─────────────────────────────────────────────────────────

/// Width of the palette box in CSS px.
pub const PANEL_WIDTH: f32 = 560.0;

/// Height of the input row in CSS px.
const INPUT_H: f32 = 40.0;

/// Height of a single result row in CSS px.
const ROW_H: f32 = 34.0;

/// Maximum number of result rows shown at once (the list scrolls beyond this).
const MAX_VISIBLE_ROWS: usize = 9;

/// Distance from the top of the window to the palette box, in CSS px.
const TOP_MARGIN: f32 = 90.0;

/// Inner horizontal padding.
const PAD: f32 = 12.0;

const SCRIM: Color = Color { r: 0, g: 0, b: 0, a: 120 };
const PANEL_BG: Color = Color { r: 24, g: 24, b: 30, a: 252 };
const PANEL_BORDER: Color = Color { r: 70, g: 70, b: 86, a: 255 };
const INPUT_BG: Color = Color { r: 16, g: 16, b: 21, a: 255 };
const ROW_SEL_BG: Color = Color { r: 48, g: 58, b: 88, a: 255 };
const SEPARATOR: Color = Color { r: 38, g: 38, b: 46, a: 255 };
const TEXT_BRIGHT: Color = Color { r: 228, g: 228, b: 236, a: 255 };
const TEXT_DIM: Color = Color { r: 140, g: 140, b: 152, a: 255 };
const TEXT_URL: Color = Color { r: 112, g: 152, b: 222, a: 255 };
const TEXT_HINT: Color = Color { r: 120, g: 124, b: 138, a: 255 };
const ICON_CMD: Color = Color { r: 150, g: 180, b: 255, a: 255 };
const ICON_BOOKMARK: Color = Color { r: 240, g: 196, b: 92, a: 255 };
const ICON_HISTORY: Color = Color { r: 140, g: 200, b: 150, a: 255 };

const FONT_SZ: f32 = 13.0;
const FONT_SZ_SM: f32 = 11.0;
const RADIUS: f32 = 8.0;

// ── Command actions ─────────────────────────────────────────────────────────

/// A built-in browser action invokable from the palette.
///
/// Each variant maps 1:1 to an existing shell command; the shell's
/// `activate_palette` translates it into the matching method call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteAction {
    /// Open a new tab.
    NewTab,
    /// Close the active tab.
    CloseTab,
    /// Reload the current page.
    Reload,
    /// Navigate back in history.
    NavigateBack,
    /// Navigate forward in history.
    NavigateForward,
    /// Open the find-on-page bar.
    FindOnPage,
    /// Focus the address bar.
    OpenAddressBar,
    /// Toggle the bookmark manager panel.
    ToggleBookmarks,
    /// Bookmark the current page.
    BookmarkCurrentPage,
    /// Toggle the vertical tab sidebar.
    ToggleVerticalTabs,
    /// Toggle the DevTools JS console.
    ToggleDevConsole,
    /// Toggle the privacy shields panel.
    ToggleShields,
    /// Toggle Vim navigation mode.
    ToggleVimMode,
}

impl PaletteAction {
    /// Human-readable label shown in the result row.
    pub fn label(self) -> &'static str {
        match self {
            PaletteAction::NewTab => "New Tab",
            PaletteAction::CloseTab => "Close Tab",
            PaletteAction::Reload => "Reload Page",
            PaletteAction::NavigateBack => "Back",
            PaletteAction::NavigateForward => "Forward",
            PaletteAction::FindOnPage => "Find on Page",
            PaletteAction::OpenAddressBar => "Open Address Bar",
            PaletteAction::ToggleBookmarks => "Toggle Bookmarks",
            PaletteAction::BookmarkCurrentPage => "Bookmark This Page",
            PaletteAction::ToggleVerticalTabs => "Toggle Vertical Tabs",
            PaletteAction::ToggleDevConsole => "Toggle DevTools Console",
            PaletteAction::ToggleShields => "Toggle Shields",
            PaletteAction::ToggleVimMode => "Toggle Vim Mode",
        }
    }

    /// Keyboard-shortcut hint rendered right-aligned in the row (`""` if none).
    pub fn shortcut(self) -> &'static str {
        match self {
            PaletteAction::NewTab => "Ctrl+T",
            PaletteAction::CloseTab => "Ctrl+W",
            PaletteAction::Reload => "Ctrl+R",
            PaletteAction::NavigateBack => "Alt+←",
            PaletteAction::NavigateForward => "Alt+→",
            PaletteAction::FindOnPage => "Ctrl+F",
            PaletteAction::OpenAddressBar => "Ctrl+L",
            PaletteAction::ToggleBookmarks => "Ctrl+Shift+O",
            PaletteAction::BookmarkCurrentPage => "Ctrl+D",
            PaletteAction::ToggleVerticalTabs => "Ctrl+B",
            PaletteAction::ToggleDevConsole => "F12",
            PaletteAction::ToggleShields => "Ctrl+Shift+S",
            PaletteAction::ToggleVimMode => "Ctrl+Alt+V",
        }
    }

    /// The full curated command list, in display order (shown first when the
    /// query is empty).
    pub fn all() -> &'static [PaletteAction] {
        &[
            PaletteAction::NewTab,
            PaletteAction::CloseTab,
            PaletteAction::Reload,
            PaletteAction::NavigateBack,
            PaletteAction::NavigateForward,
            PaletteAction::FindOnPage,
            PaletteAction::OpenAddressBar,
            PaletteAction::ToggleBookmarks,
            PaletteAction::BookmarkCurrentPage,
            PaletteAction::ToggleVerticalTabs,
            PaletteAction::ToggleDevConsole,
            PaletteAction::ToggleShields,
            PaletteAction::ToggleVimMode,
        ]
    }
}

// ── Items ───────────────────────────────────────────────────────────────────

/// What kind of target a palette item represents (drives the row icon and the
/// activation path in the shell).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaletteKind {
    /// A built-in browser command.
    Command(PaletteAction),
    /// A saved bookmark — `url` carries the navigation target.
    Bookmark,
    /// A history entry — `url` carries the navigation target.
    History,
}

/// A single searchable entry in the palette.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteItem {
    /// Discriminates command vs. bookmark vs. history.
    pub kind: PaletteKind,
    /// Primary label (command name or page title).
    pub title: String,
    /// Navigation URL for `Bookmark`/`History` items; empty for commands.
    pub url: String,
}

impl PaletteItem {
    /// Build a command item.
    pub fn command(action: PaletteAction) -> Self {
        Self {
            kind: PaletteKind::Command(action),
            title: action.label().to_owned(),
            url: String::new(),
        }
    }

    /// Build a bookmark item (falls back to the URL when the title is empty).
    pub fn bookmark(title: String, url: String) -> Self {
        let title = if title.is_empty() { url.clone() } else { title };
        Self { kind: PaletteKind::Bookmark, title, url }
    }

    /// Build a history item (falls back to the URL when the title is empty).
    pub fn history(title: String, url: String) -> Self {
        let title = if title.is_empty() { url.clone() } else { title };
        Self { kind: PaletteKind::History, title, url }
    }

    /// The text fuzzy-matched against the query: title plus URL so that typing a
    /// domain finds a bookmark/history page even when its title differs.
    fn haystack(&self) -> String {
        if self.url.is_empty() {
            self.title.clone()
        } else {
            format!("{} {}", self.title, self.url)
        }
    }
}

// ── State ─────────────────────────────────────────────────────────────────────

/// Command palette modal state.
pub struct CommandPalette {
    /// `true` while the palette overlay is visible (modal).
    pub visible: bool,
    /// Current fuzzy-search query.
    pub query: String,
    /// Index into the *filtered* result list of the highlighted row.
    pub selected: usize,
    /// First visible row index into the filtered list (vertical scroll offset
    /// in whole rows; keeps the selection on screen).
    pub scroll_row: usize,
    /// All searchable items — commands followed by bookmarks/history; refreshed
    /// every time the palette is opened.
    pub items: Vec<PaletteItem>,
}

impl CommandPalette {
    /// Create a hidden palette with the curated command list pre-loaded.
    pub fn new() -> Self {
        let items = PaletteAction::all().iter().copied().map(PaletteItem::command).collect();
        Self { visible: false, query: String::new(), selected: 0, scroll_row: 0, items }
    }

    /// Open the palette, resetting the query and selection.
    pub fn open(&mut self) {
        self.visible = true;
        self.query.clear();
        self.selected = 0;
        self.scroll_row = 0;
    }

    /// Close the palette.
    pub fn close(&mut self) {
        self.visible = false;
    }

    /// Toggle visibility; opening resets transient state.
    pub fn toggle(&mut self) {
        if self.visible {
            self.close();
        } else {
            self.open();
        }
    }

    /// Replace the item list (commands + bookmarks + history) and clamp the
    /// selection. The shell calls this on open and whenever the query changes
    /// (history results depend on the query).
    pub fn set_items(&mut self, items: Vec<PaletteItem>) {
        self.items = items;
        self.clamp_selection();
    }

    /// Append typed text to the query and reset the selection to the top.
    pub fn append(&mut self, text: &str) {
        self.query.push_str(text);
        self.selected = 0;
        self.scroll_row = 0;
    }

    /// Delete the last character of the query.
    pub fn backspace(&mut self) {
        self.query.pop();
        self.selected = 0;
        self.scroll_row = 0;
    }

    /// Indices into `items` matching the current query, best match first.
    ///
    /// An empty query returns every item in insertion order. A non-empty query
    /// keeps only fuzzy-matching items, sorted by descending score with the
    /// original order as a stable tiebreaker.
    pub fn filtered(&self) -> Vec<usize> {
        if self.query.trim().is_empty() {
            return (0..self.items.len()).collect();
        }
        let needle = self.query.trim();
        let mut scored: Vec<(usize, i32)> = self
            .items
            .iter()
            .enumerate()
            .filter_map(|(i, item)| fuzzy_score(needle, &item.haystack()).map(|s| (i, s)))
            .collect();
        // Stable sort by score descending; equal scores keep insertion order.
        scored.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        scored.into_iter().map(|(i, _)| i).collect()
    }

    /// Move the selection down by one (clamped to the last result).
    pub fn select_next(&mut self) {
        let n = self.filtered().len();
        if n == 0 {
            return;
        }
        self.selected = (self.selected + 1).min(n - 1);
        self.ensure_visible();
    }

    /// Move the selection up by one (clamped to the first result).
    pub fn select_prev(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
        self.ensure_visible();
    }

    /// The currently highlighted item index into `items`, if any result exists.
    pub fn selected_item(&self) -> Option<&PaletteItem> {
        let filtered = self.filtered();
        filtered.get(self.selected).and_then(|&i| self.items.get(i))
    }

    /// Clamp the selection to the current result count.
    fn clamp_selection(&mut self) {
        let n = self.filtered().len();
        if n == 0 {
            self.selected = 0;
        } else if self.selected >= n {
            self.selected = n - 1;
        }
        self.ensure_visible();
    }

    /// Adjust `scroll_row` so the selected row stays inside the visible window.
    fn ensure_visible(&mut self) {
        if self.selected < self.scroll_row {
            self.scroll_row = self.selected;
        } else if self.selected >= self.scroll_row + MAX_VISIBLE_ROWS {
            self.scroll_row = self.selected + 1 - MAX_VISIBLE_ROWS;
        }
    }
}

impl Default for CommandPalette {
    fn default() -> Self {
        Self::new()
    }
}

// ── Fuzzy matching ────────────────────────────────────────────────────────────

/// Score `haystack` against `needle` as a case-insensitive subsequence match.
///
/// Returns `None` when `needle` is not a subsequence of `haystack`. Otherwise a
/// higher score is a better match. Bonuses: consecutive matched characters, a
/// match at a word boundary (start, or after a space / `.` / `/` / `-` / `_`),
/// and a shorter haystack. This makes acronym-style queries (`nt` → *New Tab*)
/// and prefix queries rank above scattered matches.
pub fn fuzzy_score(needle: &str, haystack: &str) -> Option<i32> {
    let needle = needle.trim();
    if needle.is_empty() {
        return Some(0);
    }
    let hay: Vec<char> = haystack.chars().flat_map(char::to_lowercase).collect();
    let pat: Vec<char> = needle.chars().flat_map(char::to_lowercase).collect();

    let mut score = 0i32;
    let mut hi = 0usize;
    let mut prev_matched = false;
    for &pc in &pat {
        // Advance through the haystack to the next matching char.
        let mut found = false;
        while hi < hay.len() {
            let hc = hay[hi];
            if hc == pc {
                // Base point for the match.
                score += 1;
                // Word-boundary bonus.
                let boundary = hi == 0
                    || matches!(hay[hi - 1], ' ' | '.' | '/' | '-' | '_' | ':');
                if boundary {
                    score += 8;
                }
                // Consecutive-run bonus.
                if prev_matched {
                    score += 5;
                }
                hi += 1;
                prev_matched = true;
                found = true;
                break;
            }
            hi += 1;
            prev_matched = false;
        }
        if !found {
            return None;
        }
    }
    // Prefer shorter haystacks (tiny penalty for length).
    score -= (hay.len() as i32) / 32;
    Some(score)
}

// ── Hit-testing ───────────────────────────────────────────────────────────────

/// Result of a click inside the modal palette.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaletteHit {
    /// Click landed on a result row — carries the index into the *filtered*
    /// list (use [`CommandPalette::filtered`] to resolve).
    Row(usize),
    /// Click inside the box but not on a row (e.g. the input bar).
    Inside,
    /// Click on the dimmed scrim outside the box — closes the palette.
    Dismiss,
}

/// Compute the palette box's top-left corner for a `viewport_w`-wide window.
fn box_origin(viewport_w: f32) -> (f32, f32) {
    let ax = ((viewport_w - PANEL_WIDTH) * 0.5).max(0.0);
    (ax, TOP_MARGIN)
}

/// Total box height for `row_count` visible rows.
fn box_height(row_count: usize) -> f32 {
    let rows = row_count.clamp(1, MAX_VISIBLE_ROWS);
    INPUT_H + rows as f32 * ROW_H + 2.0
}

/// Hit-test a click at CSS-px `(x, y)` against the modal palette in a
/// `viewport_w`×`viewport_h` window.
pub fn hit_test(palette: &CommandPalette, x: f32, y: f32, viewport_w: f32) -> PaletteHit {
    let (ax, ay) = box_origin(viewport_w);
    let rows = palette.filtered().len();
    let h = box_height(rows);
    if x < ax || x >= ax + PANEL_WIDTH || y < ay || y >= ay + h {
        return PaletteHit::Dismiss;
    }
    let ly = y - ay;
    if ly < INPUT_H {
        return PaletteHit::Inside;
    }
    let row_in_view = ((ly - INPUT_H) / ROW_H) as usize;
    let abs = palette.scroll_row + row_in_view;
    if abs < rows {
        return PaletteHit::Row(abs);
    }
    PaletteHit::Inside
}

// ── Rendering ─────────────────────────────────────────────────────────────────

/// Build the display list for the modal palette over a `viewport_w`×`viewport_h`
/// window (viewport-locked, not scrolled with the page).
pub fn build_panel(palette: &CommandPalette, viewport_w: f32, viewport_h: f32) -> DisplayList {
    let mut out = DisplayList::with_capacity(32);
    let radii = uniform_radii(RADIUS);

    // Full-window dimming scrim (also swallows outside clicks via hit-test).
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(0.0, 0.0, viewport_w, viewport_h),
        color: SCRIM,
    });

    let (ax, ay) = box_origin(viewport_w);
    let filtered = palette.filtered();
    let h = box_height(filtered.len());

    // Box background + 1px border.
    out.push(DisplayCommand::FillRoundedRect {
        rect: Rect::new(ax, ay, PANEL_WIDTH, h),
        radii,
        color: PANEL_BORDER,
    });
    out.push(DisplayCommand::FillRoundedRect {
        rect: Rect::new(ax + 1.0, ay + 1.0, PANEL_WIDTH - 2.0, h - 2.0),
        radii,
        color: PANEL_BG,
    });

    // Input row.
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(ax + 1.0, ay + 1.0, PANEL_WIDTH - 2.0, INPUT_H - 1.0),
        color: INPUT_BG,
    });
    out.push(text(
        ax + PAD,
        ay + (INPUT_H - FONT_SZ * 1.3) * 0.5,
        18.0,
        "›",
        FONT_SZ + 3.0,
        TEXT_DIM,
        FontWeight::BOLD,
    ));
    let (q_text, q_col) = if palette.query.is_empty() {
        ("Type a command, bookmark or history…".to_owned(), TEXT_DIM)
    } else {
        (palette.query.clone(), TEXT_BRIGHT)
    };
    out.push(text(
        ax + PAD + 20.0,
        ay + (INPUT_H - FONT_SZ * 1.3) * 0.5,
        PANEL_WIDTH - 2.0 * PAD - 20.0,
        &q_text,
        FONT_SZ,
        q_col,
        FontWeight::NORMAL,
    ));

    // Result list (clipped to the box body).
    let list_top = ay + INPUT_H;
    let list_h = h - INPUT_H - 1.0;
    out.push(DisplayCommand::PushClipRect {
        rect: Rect::new(ax + 1.0, list_top, PANEL_WIDTH - 2.0, list_h),
    });

    if filtered.is_empty() {
        out.push(text(
            ax + PAD + 8.0,
            list_top + 10.0,
            PANEL_WIDTH - 2.0 * PAD,
            "No results",
            FONT_SZ,
            TEXT_DIM,
            FontWeight::NORMAL,
        ));
    }

    let end = (palette.scroll_row + MAX_VISIBLE_ROWS).min(filtered.len());
    for (vis, &item_idx) in filtered[palette.scroll_row..end].iter().enumerate() {
        let Some(item) = palette.items.get(item_idx) else { continue };
        let abs_row = palette.scroll_row + vis;
        let ry = list_top + vis as f32 * ROW_H;

        if abs_row == palette.selected {
            out.push(DisplayCommand::FillRect {
                rect: Rect::new(ax + 1.0, ry, PANEL_WIDTH - 2.0, ROW_H),
                color: ROW_SEL_BG,
            });
        }

        // Leading icon glyph by kind.
        let (icon, icon_col) = match item.kind {
            PaletteKind::Command(_) => ("»", ICON_CMD),
            PaletteKind::Bookmark => ("★", ICON_BOOKMARK),
            PaletteKind::History => ("◷", ICON_HISTORY),
        };
        out.push(text(
            ax + PAD,
            ry + (ROW_H - FONT_SZ * 1.3) * 0.5,
            18.0,
            icon,
            FONT_SZ,
            icon_col,
            FontWeight::NORMAL,
        ));

        // Title.
        let title_x = ax + PAD + 22.0;
        let title_w = PANEL_WIDTH - 2.0 * PAD - 22.0 - 140.0;
        out.push(text(
            title_x,
            ry + (ROW_H - FONT_SZ * 1.3) * 0.5,
            title_w,
            &truncate(&item.title, 52),
            FONT_SZ,
            TEXT_BRIGHT,
            FontWeight::NORMAL,
        ));

        // Trailing hint: shortcut for commands, host for nav items.
        let (hint, hint_col) = match item.kind {
            PaletteKind::Command(a) => (a.shortcut().to_owned(), TEXT_HINT),
            PaletteKind::Bookmark | PaletteKind::History => (host_of(&item.url), TEXT_URL),
        };
        if !hint.is_empty() {
            out.push(text(
                ax + PANEL_WIDTH - PAD - 134.0,
                ry + (ROW_H - FONT_SZ_SM * 1.3) * 0.5,
                134.0,
                &truncate(&hint, 22),
                FONT_SZ_SM,
                hint_col,
                FontWeight::NORMAL,
            ));
        }

        // Row separator.
        out.push(DisplayCommand::FillRect {
            rect: Rect::new(ax + 1.0, ry + ROW_H - 1.0, PANEL_WIDTH - 2.0, 1.0),
            color: SEPARATOR,
        });
    }

    out.push(DisplayCommand::PopClip);
    out
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Build a `DrawText` command with the palette's default font settings.
fn text(x: f32, y: f32, w: f32, s: &str, size: f32, color: Color, weight: FontWeight) -> DisplayCommand {
    DisplayCommand::DrawText {
        rect: Rect::new(x, y, w.max(0.0), size * 1.4),
        text: s.to_owned(),
        font_size: size,
        color,
        font_family: Vec::new(),
        font_weight: weight,
        font_style: FontStyle::Normal,
        font_variation_axes: Vec::new(),
        tab_size: 0.0,
    }
}

/// Uniform corner radii.
fn uniform_radii(r: f32) -> CornerRadii {
    CornerRadii {
        tl: r, tl_y: r,
        tr: r, tr_y: r,
        br: r, br_y: r,
        bl: r, bl_y: r,
    }
}

/// Truncate a label to at most `max_chars` characters, appending "…" if cut.
fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_owned();
    }
    let mut out: String = s.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Extract the host portion of a URL for the trailing hint (best-effort; falls
/// back to the raw string when there is no `scheme://host` shape).
fn host_of(url: &str) -> String {
    let after_scheme = url.split_once("://").map_or(url, |(_, rest)| rest);
    let host = after_scheme.split(['/', '?', '#']).next().unwrap_or(after_scheme);
    host.trim_start_matches("www.").to_owned()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn with_items(p: &mut CommandPalette) {
        let mut items: Vec<PaletteItem> =
            PaletteAction::all().iter().copied().map(PaletteItem::command).collect();
        items.push(PaletteItem::bookmark(
            "Rust Programming Language".into(),
            "https://www.rust-lang.org/".into(),
        ));
        items.push(PaletteItem::history(
            "Example Domain".into(),
            "https://example.com/path".into(),
        ));
        p.set_items(items);
    }

    // ── State ──────────────────────────────────────────────────────────────────

    #[test]
    fn new_palette_hidden_with_commands() {
        let p = CommandPalette::new();
        assert!(!p.visible);
        assert_eq!(p.items.len(), PaletteAction::all().len());
    }

    #[test]
    fn open_resets_query_and_selection() {
        let mut p = CommandPalette::new();
        p.query = "stale".into();
        p.selected = 5;
        p.open();
        assert!(p.visible);
        assert!(p.query.is_empty());
        assert_eq!(p.selected, 0);
    }

    #[test]
    fn toggle_flips_visibility() {
        let mut p = CommandPalette::new();
        p.toggle();
        assert!(p.visible);
        p.toggle();
        assert!(!p.visible);
    }

    #[test]
    fn append_and_backspace() {
        let mut p = CommandPalette::new();
        p.append("ne");
        p.append("w");
        assert_eq!(p.query, "new");
        p.backspace();
        assert_eq!(p.query, "ne");
    }

    // ── Fuzzy matching ─────────────────────────────────────────────────────────

    #[test]
    fn fuzzy_subsequence_matches() {
        assert!(fuzzy_score("nt", "New Tab").is_some());
        assert!(fuzzy_score("reload", "Reload Page").is_some());
        assert!(fuzzy_score("xyz", "New Tab").is_none());
    }

    #[test]
    fn fuzzy_empty_query_matches_all() {
        assert_eq!(fuzzy_score("", "anything"), Some(0));
    }

    #[test]
    fn fuzzy_word_boundary_beats_scattered() {
        // "nt" as acronym (New Tab) should beat the same letters mid-word.
        let acronym = fuzzy_score("nt", "New Tab").unwrap();
        let scattered = fuzzy_score("nt", "Inkpot").unwrap();
        assert!(acronym > scattered, "acronym {acronym} vs scattered {scattered}");
    }

    #[test]
    fn fuzzy_is_case_insensitive() {
        assert!(fuzzy_score("RUST", "rust-lang").is_some());
        assert!(fuzzy_score("rust", "RUST-LANG").is_some());
    }

    // ── Filtering / selection ──────────────────────────────────────────────────

    #[test]
    fn empty_query_shows_all_in_order() {
        let mut p = CommandPalette::new();
        with_items(&mut p);
        let f = p.filtered();
        assert_eq!(f.len(), p.items.len());
        assert_eq!(f[0], 0);
    }

    #[test]
    fn query_filters_and_ranks() {
        let mut p = CommandPalette::new();
        with_items(&mut p);
        p.append("new tab");
        let f = p.filtered();
        assert!(!f.is_empty());
        // Top match should be the New Tab command.
        let top = &p.items[f[0]];
        assert_eq!(top.kind, PaletteKind::Command(PaletteAction::NewTab));
    }

    #[test]
    fn query_matches_bookmark_by_url() {
        let mut p = CommandPalette::new();
        with_items(&mut p);
        p.append("rust-lang");
        let item = p.selected_item().unwrap();
        assert_eq!(item.kind, PaletteKind::Bookmark);
    }

    #[test]
    fn no_results_for_garbage_query() {
        let mut p = CommandPalette::new();
        with_items(&mut p);
        p.append("zzzqqq___");
        assert!(p.filtered().is_empty());
        assert!(p.selected_item().is_none());
    }

    #[test]
    fn select_next_prev_clamped() {
        let mut p = CommandPalette::new();
        with_items(&mut p);
        let n = p.filtered().len();
        p.select_prev();
        assert_eq!(p.selected, 0);
        for _ in 0..(n + 5) {
            p.select_next();
        }
        assert_eq!(p.selected, n - 1);
    }

    #[test]
    fn ensure_visible_scrolls_window() {
        let mut p = CommandPalette::new();
        with_items(&mut p);
        for _ in 0..(MAX_VISIBLE_ROWS + 2) {
            p.select_next();
        }
        // Selected row must be within the visible window.
        assert!(p.selected >= p.scroll_row);
        assert!(p.selected < p.scroll_row + MAX_VISIBLE_ROWS);
    }

    #[test]
    fn set_items_clamps_stale_selection() {
        let mut p = CommandPalette::new();
        with_items(&mut p);
        p.selected = 100;
        p.set_items(vec![PaletteItem::command(PaletteAction::NewTab)]);
        assert_eq!(p.selected, 0);
    }

    // ── Hit-testing ────────────────────────────────────────────────────────────

    #[test]
    fn hit_outside_box_dismisses() {
        let mut p = CommandPalette::new();
        with_items(&mut p);
        // Click far below/left of the box.
        assert_eq!(hit_test(&p, 5.0, 5.0, 1024.0), PaletteHit::Dismiss);
    }

    #[test]
    fn hit_input_row_is_inside() {
        let mut p = CommandPalette::new();
        with_items(&mut p);
        let (ax, ay) = box_origin(1024.0);
        let hit = hit_test(&p, ax + 50.0, ay + INPUT_H * 0.5, 1024.0);
        assert_eq!(hit, PaletteHit::Inside);
    }

    #[test]
    fn hit_first_result_row() {
        let mut p = CommandPalette::new();
        with_items(&mut p);
        let (ax, ay) = box_origin(1024.0);
        let y = ay + INPUT_H + ROW_H * 0.5;
        assert_eq!(hit_test(&p, ax + 50.0, y, 1024.0), PaletteHit::Row(0));
    }

    // ── Rendering ──────────────────────────────────────────────────────────────

    #[test]
    fn build_panel_balanced_clip() {
        let mut p = CommandPalette::new();
        with_items(&mut p);
        p.visible = true;
        let dl = build_panel(&p, 1024.0, 720.0);
        let push = dl.iter().filter(|c| matches!(c, DisplayCommand::PushClipRect { .. })).count();
        let pop = dl.iter().filter(|c| matches!(c, DisplayCommand::PopClip)).count();
        assert_eq!(push, pop);
        assert!(!dl.is_empty());
    }

    #[test]
    fn build_panel_draws_scrim_and_commands() {
        let mut p = CommandPalette::new();
        with_items(&mut p);
        p.visible = true;
        let dl = build_panel(&p, 1024.0, 720.0);
        // Scrim is the first command, full viewport.
        assert!(matches!(
            dl.first(),
            Some(DisplayCommand::FillRect { rect, .. })
                if rect.width == 1024.0 && rect.height == 720.0
        ));
        let has = |needle: &str| {
            dl.iter().any(|c| matches!(c, DisplayCommand::DrawText { text, .. } if text == needle))
        };
        assert!(has("New Tab"));
    }

    #[test]
    fn build_panel_empty_shows_placeholder() {
        let mut p = CommandPalette::new();
        with_items(&mut p);
        p.visible = true;
        p.append("zzzqqq___");
        let dl = build_panel(&p, 1024.0, 720.0);
        let has_placeholder = dl
            .iter()
            .any(|c| matches!(c, DisplayCommand::DrawText { text, .. } if text == "No results"));
        assert!(has_placeholder);
    }

    #[test]
    fn host_of_strips_scheme_and_www() {
        assert_eq!(host_of("https://www.rust-lang.org/learn"), "rust-lang.org");
        assert_eq!(host_of("http://example.com"), "example.com");
        assert_eq!(host_of("about:blank"), "about:blank");
    }
}
