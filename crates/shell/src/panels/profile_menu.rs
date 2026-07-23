//! Profile switcher dropdown (DS-14): anchored below the avatar button in
//! the permanent toolbar's left cluster (`toolbar::avatar_x()`). Lists every
//! profile as a colour dot + name; clicking one switches the active profile.
//!
//! Scope (DS-14, `docs/tasks/p1-design-v3.md`): this slice changes only the
//! active-profile pointer and the chrome's visual signature (avatar
//! colour/letter, `Palette::accent`). Per-profile *data* isolation —
//! separate history/cookie-jar/bookmarks per profile — is explicitly NOT
//! implemented here; see DS-16.

use lumen_core::geom::Rect;
use lumen_layout::{Color, FontStyle, FontWeight};
use lumen_paint::{CornerRadii, DisplayCommand, DisplayList};

use crate::panels::themes::Palette;
use crate::theme_tokens::{self, radius};

// ── Visual constants ─────────────────────────────────────────────────────────

/// Width of the dropdown in CSS px (`.profile-menu` in the design reference).
pub const MENU_W: f32 = 190.0;
/// Height of one profile row.
const ROW_H: f32 = 30.0;
/// Padding around the row list, inside the menu border.
const MENU_PAD: f32 = 6.0;
/// Diameter of the colour dot preceding each profile name.
const DOT_SZ: f32 = 9.0;
/// Gap between the toolbar's bottom edge and the dropdown (mirrors
/// `shields_panel::PANEL_TOP_OFFSET`).
const MENU_TOP_OFFSET: f32 = 4.0;
const FONT_SZ: f32 = 12.0;
const MENU_RADIUS: f32 = radius::LG;

// ── Default profile seed ─────────────────────────────────────────────────────

/// Default profiles created on first run (DS-14 step 1): `(name, storage
/// slug, accent colour)`. Order also fixes each profile's row order in the
/// dropdown; `[0]` becomes active by default. The storage slug is only a
/// placeholder `storage_path` for the registry row — no per-profile storage
/// actually lives there yet (DS-16).
pub const DEFAULT_PROFILES: [(&str, &str, Color); 4] = [
    ("Личный", "personal", theme_tokens::profile::PERSONAL),
    ("Рабочий", "work", theme_tokens::profile::WORK),
    ("Анонимный", "anonymous", theme_tokens::profile::ANONYMOUS),
    ("Гость", "guest", theme_tokens::profile::GUEST),
];

/// Accent colour for a profile by name, falling back to a cyclic default (by
/// `index`) for any profile outside the seeded four — DS-14 ships no UI to
/// create further profiles, but nothing in the registry itself forbids it.
#[must_use]
pub fn color_for_profile(name: &str, index: usize) -> Color {
    DEFAULT_PROFILES
        .iter()
        .find(|(n, ..)| *n == name)
        .map(|(_, _, c)| *c)
        .unwrap_or_else(|| DEFAULT_PROFILES[index % DEFAULT_PROFILES.len()].2)
}

// ── Data types ────────────────────────────────────────────────────────────────

/// One profile row as rendered in the dropdown.
#[derive(Debug, Clone)]
pub struct ProfileEntry {
    /// `lumen_storage::Profile::id`.
    pub id: i64,
    /// Display name.
    pub name: String,
    /// Row dot / avatar accent colour.
    pub color: Color,
}

// ── Panel state ───────────────────────────────────────────────────────────────

/// Profile switcher dropdown state.
pub struct ProfileMenuPanel {
    /// `true` while the dropdown is visible. Toggled by clicking the
    /// toolbar avatar (`ToolbarHit::Profile`).
    pub visible: bool,
    /// Cached profile list — refreshed from `ProfileRegistry::list_all` each
    /// time the dropdown opens.
    pub entries: Vec<ProfileEntry>,
    /// Id of the currently active profile, if any.
    pub active_id: Option<i64>,
}

impl ProfileMenuPanel {
    /// Create a new hidden panel with an empty profile list.
    pub fn new() -> Self {
        Self { visible: false, entries: Vec::new(), active_id: None }
    }

    /// Flip dropdown visibility.
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }

    /// Replace the cached profile list (call after opening the dropdown or
    /// after any registry mutation).
    pub fn set_entries(&mut self, entries: Vec<ProfileEntry>) {
        self.entries = entries;
    }

    /// Mark `id` as the active profile.
    pub fn set_active(&mut self, id: Option<i64>) {
        self.active_id = id;
    }

    /// The cached entry matching `active_id`, if any — drives the toolbar
    /// avatar colour/letter and the chrome accent override.
    #[must_use]
    pub fn active_entry(&self) -> Option<&ProfileEntry> {
        let id = self.active_id?;
        self.entries.iter().find(|e| e.id == id)
    }
}

impl Default for ProfileMenuPanel {
    fn default() -> Self {
        Self::new()
    }
}

// ── Hit-testing ───────────────────────────────────────────────────────────────

/// Result of a click inside the profile dropdown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileMenuHit {
    /// Switch to the profile with the given id.
    SwitchTo(i64),
    /// Clicked inside the dropdown but on a non-actionable area (padding).
    Empty,
}

/// Hit-test a click at CSS-px `(x, y)` against the profile dropdown.
///
/// Returns `None` when the click is outside the dropdown. `avatar_x` is
/// `toolbar::avatar_x()`; `chrome_h` is `toolbar::CHROME_H` — the dropdown
/// is anchored below the toolbar, left-aligned with the avatar button.
pub fn hit_test(
    panel: &ProfileMenuPanel,
    x: f32,
    y: f32,
    avatar_x: f32,
    chrome_h: f32,
) -> Option<ProfileMenuHit> {
    let (mx, my) = menu_origin(avatar_x, chrome_h);
    let menu_h = menu_height(panel);
    if x < mx || x >= mx + MENU_W || y < my || y >= my + menu_h {
        return None;
    }
    if y < my + MENU_PAD {
        return Some(ProfileMenuHit::Empty);
    }
    let row_idx = ((y - my - MENU_PAD) / ROW_H) as usize;
    match panel.entries.get(row_idx) {
        Some(entry) => Some(ProfileMenuHit::SwitchTo(entry.id)),
        None => Some(ProfileMenuHit::Empty),
    }
}

// ── Rendering ─────────────────────────────────────────────────────────────────

/// Build the display list for the profile dropdown.
///
/// Anchored at `(avatar_x, chrome_h + MENU_TOP_OFFSET)`. Each row shows a
/// colour dot + profile name; the active row gets a highlighted background
/// and bold text.
pub fn build_panel(
    panel: &ProfileMenuPanel,
    avatar_x: f32,
    chrome_h: f32,
    pal: &Palette,
) -> DisplayList {
    let (mx, my) = menu_origin(avatar_x, chrome_h);
    let menu_h = menu_height(panel);
    let mut out = DisplayList::with_capacity(4 + panel.entries.len() * 3);

    out.push(DisplayCommand::FillRoundedRect {
        rect: Rect::new(mx, my, MENU_W, menu_h),
        radii: uniform_radii(MENU_RADIUS),
        color: pal.overlay_border,
    });
    out.push(DisplayCommand::FillRoundedRect {
        rect: Rect::new(mx + 1.0, my + 1.0, MENU_W - 2.0, menu_h - 2.0),
        radii: uniform_radii(MENU_RADIUS - 1.0),
        color: pal.overlay_bg,
    });

    for (i, entry) in panel.entries.iter().enumerate() {
        let row_y = my + MENU_PAD + i as f32 * ROW_H;
        let is_active = panel.active_id == Some(entry.id);

        if is_active {
            out.push(DisplayCommand::FillRoundedRect {
                rect: Rect::new(mx + 3.0, row_y, MENU_W - 6.0, ROW_H),
                radii: uniform_radii(radius::MD),
                color: pal.item_selected_bg,
            });
        }

        let dot_y = row_y + (ROW_H - DOT_SZ) * 0.5;
        out.push(DisplayCommand::FillRoundedRect {
            rect: Rect::new(mx + MENU_PAD + 4.0, dot_y, DOT_SZ, DOT_SZ),
            radii: uniform_radii(DOT_SZ * 0.5),
            color: entry.color,
        });

        let text_x = mx + MENU_PAD + 4.0 + DOT_SZ + 8.0;
        let text_y = row_y + (ROW_H - FONT_SZ * 1.3) * 0.5;
        out.push(DisplayCommand::DrawText {
            rect: Rect::new(text_x, text_y, (mx + MENU_W - MENU_PAD - text_x).max(0.0), FONT_SZ * 1.3),
            text: entry.name.clone(),
            font_size: FONT_SZ,
            color: if is_active { pal.text } else { pal.text_dim },
            font_family: Vec::new(),
            font_weight: if is_active { FontWeight::BOLD } else { FontWeight::NORMAL },
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

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Top-left corner of the dropdown in CSS px.
fn menu_origin(avatar_x: f32, chrome_h: f32) -> (f32, f32) {
    (avatar_x, chrome_h + MENU_TOP_OFFSET)
}

/// Total dropdown height for the current entry count.
fn menu_height(panel: &ProfileMenuPanel) -> f32 {
    MENU_PAD * 2.0 + panel.entries.len() as f32 * ROW_H
}

fn uniform_radii(r: f32) -> CornerRadii {
    CornerRadii { tl: r, tl_y: r, tr: r, tr_y: r, br: r, br_y: r, bl: r, bl_y: r }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const AVATAR_X: f32 = 10.0;
    const CHROME_H: f32 = 72.0;

    fn make_panel() -> ProfileMenuPanel {
        let mut p = ProfileMenuPanel::new();
        p.visible = true;
        p.entries = vec![
            ProfileEntry { id: 1, name: "Личный".to_owned(), color: theme_tokens::profile::PERSONAL },
            ProfileEntry { id: 2, name: "Рабочий".to_owned(), color: theme_tokens::profile::WORK },
            ProfileEntry {
                id: 3,
                name: "Анонимный".to_owned(),
                color: theme_tokens::profile::ANONYMOUS,
            },
        ];
        p.active_id = Some(1);
        p
    }

    // ── Panel state ──────────────────────────────────────────────────────────

    #[test]
    fn new_panel_hidden() {
        let p = ProfileMenuPanel::new();
        assert!(!p.visible);
        assert!(p.entries.is_empty());
        assert_eq!(p.active_id, None);
    }

    #[test]
    fn toggle_shows_and_hides() {
        let mut p = ProfileMenuPanel::new();
        p.toggle();
        assert!(p.visible);
        p.toggle();
        assert!(!p.visible);
    }

    #[test]
    fn active_entry_matches_active_id() {
        let p = make_panel();
        assert_eq!(p.active_entry().unwrap().name, "Личный");
    }

    #[test]
    fn active_entry_none_when_no_active_id() {
        let mut p = make_panel();
        p.active_id = None;
        assert!(p.active_entry().is_none());
    }

    #[test]
    fn active_entry_none_when_id_not_cached() {
        let mut p = make_panel();
        p.set_active(Some(999));
        assert!(p.active_entry().is_none());
    }

    #[test]
    fn set_active_updates_id() {
        let mut p = ProfileMenuPanel::new();
        p.set_active(Some(2));
        assert_eq!(p.active_id, Some(2));
    }

    // ── Hit-testing ──────────────────────────────────────────────────────────

    #[test]
    fn hit_outside_menu_returns_none() {
        let p = make_panel();
        assert_eq!(hit_test(&p, 0.0, 0.0, AVATAR_X, CHROME_H), None);
    }

    #[test]
    fn hit_first_row_switches() {
        let p = make_panel();
        let (mx, my) = menu_origin(AVATAR_X, CHROME_H);
        let hit = hit_test(&p, mx + MENU_PAD + 5.0, my + MENU_PAD + 5.0, AVATAR_X, CHROME_H);
        assert_eq!(hit, Some(ProfileMenuHit::SwitchTo(1)));
    }

    #[test]
    fn hit_second_row_switches() {
        let p = make_panel();
        let (mx, my) = menu_origin(AVATAR_X, CHROME_H);
        let y = my + MENU_PAD + ROW_H + 5.0;
        let hit = hit_test(&p, mx + 20.0, y, AVATAR_X, CHROME_H);
        assert_eq!(hit, Some(ProfileMenuHit::SwitchTo(2)));
    }

    #[test]
    fn hit_top_padding_is_empty() {
        let p = make_panel();
        let (mx, my) = menu_origin(AVATAR_X, CHROME_H);
        let hit = hit_test(&p, mx + 20.0, my + 1.0, AVATAR_X, CHROME_H);
        assert_eq!(hit, Some(ProfileMenuHit::Empty));
    }

    // ── Rendering ────────────────────────────────────────────────────────────

    #[test]
    fn build_panel_emits_commands() {
        let p = make_panel();
        let dl = build_panel(&p, AVATAR_X, CHROME_H, &Palette::DARK);
        assert!(!dl.is_empty());
    }

    #[test]
    fn build_panel_draws_all_names() {
        let p = make_panel();
        let dl = build_panel(&p, AVATAR_X, CHROME_H, &Palette::DARK);
        for name in ["Личный", "Рабочий", "Анонимный"] {
            let found = dl.iter().any(|c| {
                matches!(c, DisplayCommand::DrawText { text, .. } if text == name)
            });
            assert!(found, "missing row for {name}");
        }
    }

    #[test]
    fn build_panel_no_rows_for_empty_list() {
        let mut p = make_panel();
        p.entries.clear();
        let dl = build_panel(&p, AVATAR_X, CHROME_H, &Palette::DARK);
        let texts = dl.iter().filter(|c| matches!(c, DisplayCommand::DrawText { .. })).count();
        assert_eq!(texts, 0);
    }

    // ── Colour mapping ───────────────────────────────────────────────────────

    #[test]
    fn default_profiles_has_four_unique_names() {
        let names: std::collections::HashSet<_> =
            DEFAULT_PROFILES.iter().map(|(n, ..)| *n).collect();
        assert_eq!(names.len(), 4);
    }

    #[test]
    fn color_for_profile_matches_known_name() {
        let c = color_for_profile("Рабочий", 0);
        assert_eq!(c, theme_tokens::profile::WORK);
    }

    #[test]
    fn color_for_profile_falls_back_to_cycle() {
        let c = color_for_profile("Кастомный", 1);
        assert_eq!(c, DEFAULT_PROFILES[1].2);
    }
}
