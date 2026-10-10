//! Profile switcher dropdown state (DS-14). The dropdown itself is
//! `#profileMenu`/`#profileMenuH` in `assets/chrome/chrome.html`
//! (UX-CHROME-PANELS); this module keeps the profile list, the active id and
//! the seeded-profile lookups.
//!
//! Scope (DS-14, `docs/tasks/p1-design-v3.md`): this slice changes only the
//! active-profile pointer and the chrome's visual signature (avatar
//! colour/letter, `Palette::accent`). Per-profile *data* isolation —
//! separate history/cookie-jar/bookmarks per profile — is explicitly NOT
//! implemented here; see DS-16.

use lumen_layout::Color;

use crate::theme_tokens;

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

/// Chrome `data-profile` slug for `name` (CC-6, `docs/tasks/p1-css-chrome.md`)
/// — `None` for a profile outside the seeded four, since the engine-drawn
/// chrome's CSS only carries `body[data-profile="…"]` branches for those
/// slugs (see `assets/chrome/chrome.html`); the attribute is then omitted by
/// the caller rather than guessed, leaving the chrome on its `:root` default
/// tokens. Unlike [`color_for_profile`], no cyclic fallback — a wrong slug
/// would silently apply another profile's accent tokens, which is worse than
/// applying none.
#[must_use]
pub fn slug_for_profile(name: &str) -> Option<&'static str> {
    DEFAULT_PROFILES.iter().find(|(n, ..)| *n == name).map(|(_, slug, _)| *slug)
}

/// `true` when `name` is the seeded Anonymous profile (DS-15: draws the red
/// inset window outline). Matched by name, same as [`color_for_profile`] —
/// DS-14 ships no rename UI for the seeded four, so this stays exact-match.
#[must_use]
pub fn is_anonymous(name: &str) -> bool {
    name == DEFAULT_PROFILES[2].0
}

/// `true` when `name` is the seeded Guest profile (DS-15: desaturates the
/// whole chrome palette). See [`is_anonymous`] for the matching rationale.
#[must_use]
pub fn is_guest(name: &str) -> bool {
    name == DEFAULT_PROFILES[3].0
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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

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

    // ── DS-15: profile-kind matching ─────────────────────────────────────────

    #[test]
    fn is_anonymous_matches_seeded_name() {
        assert!(is_anonymous("Анонимный"));
        assert!(!is_anonymous("Личный"));
        assert!(!is_anonymous("Гость"));
    }

    #[test]
    fn is_guest_matches_seeded_name() {
        assert!(is_guest("Гость"));
        assert!(!is_guest("Личный"));
        assert!(!is_guest("Анонимный"));
    }

    // ── CC-6: `data-profile` slug lookup ─────────────────────────────────────

    #[test]
    fn slug_for_profile_matches_seeded_names() {
        assert_eq!(slug_for_profile("Личный"), Some("personal"));
        assert_eq!(slug_for_profile("Рабочий"), Some("work"));
        assert_eq!(slug_for_profile("Анонимный"), Some("anonymous"));
        assert_eq!(slug_for_profile("Гость"), Some("guest"));
    }

    #[test]
    fn slug_for_profile_none_for_custom_name() {
        assert_eq!(slug_for_profile("Кастомный"), None);
    }
}
