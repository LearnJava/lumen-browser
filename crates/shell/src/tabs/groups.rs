//! Tab groups (CC-6): named, colour-coded collections of adjacent tabs that
//! can be collapsed to a single chip and expanded back.
//!
//! A *tab group* bundles several tabs under one coloured label. Compared to
//! [`super::containers`] (which isolates cookies/storage), a group is a pure
//! UI-organisation construct: it has no effect on the cookie jar. Each group
//! owns a [`GroupColor`] used for the strip label and the per-tab top accent
//! bar, plus a `collapsed` flag that hides member tabs behind the group's
//! leftmost tab.
//!
//! This module ships the data types ([`GroupColor`], [`TabGroup`]) and pure
//! helpers. Integration into the tab strip (membership, collapse-aware
//! layout) lives in [`super::strip`]; SQLite persistence of group metadata
//! lives in [`lumen_storage::tab_groups`]; shell wiring is in
//! `crates/shell/src/main.rs`.

use lumen_layout::Color;

/// One of the preset tab-group colours (Chrome-compatible palette).
///
/// Derives `Copy` so the value can flow through `Copy`-typed shell command
/// enums without boxing, mirroring [`super::containers::ContainerKind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GroupColor {
    /// Neutral grey — the default for a freshly created group.
    Grey,
    /// Blue (#3B82F6).
    Blue,
    /// Red (#EF4444).
    Red,
    /// Yellow (#EAB308).
    Yellow,
    /// Green (#22C55E).
    Green,
    /// Pink (#EC4899).
    Pink,
    /// Purple (#8B5CF6).
    Purple,
    /// Cyan (#06B6D4).
    Cyan,
}

impl GroupColor {
    /// Every preset colour in palette order. Used to cycle through colours and
    /// to round-trip a [`GroupColor`] through a small integer for persistence.
    pub const ALL: [GroupColor; 8] = [
        GroupColor::Grey,
        GroupColor::Blue,
        GroupColor::Red,
        GroupColor::Yellow,
        GroupColor::Green,
        GroupColor::Pink,
        GroupColor::Purple,
        GroupColor::Cyan,
    ];

    /// Fully-opaque RGB for the strip label and the per-tab accent bar.
    #[must_use]
    pub fn color(self) -> Color {
        match self {
            GroupColor::Grey => Color { r: 0x9C, g: 0xA3, b: 0xAF, a: 255 },
            GroupColor::Blue => Color { r: 0x3B, g: 0x82, b: 0xF6, a: 255 },
            GroupColor::Red => Color { r: 0xEF, g: 0x44, b: 0x44, a: 255 },
            GroupColor::Yellow => Color { r: 0xEA, g: 0xB3, b: 0x08, a: 255 },
            GroupColor::Green => Color { r: 0x22, g: 0xC5, b: 0x5E, a: 255 },
            GroupColor::Pink => Color { r: 0xEC, g: 0x48, b: 0x99, a: 255 },
            GroupColor::Purple => Color { r: 0x8B, g: 0x5C, b: 0xF6, a: 255 },
            GroupColor::Cyan => Color { r: 0x06, g: 0xB6, b: 0xD4, a: 255 },
        }
    }

    /// Stable palette index (`0..8`), used as the persisted on-disk value.
    #[must_use]
    pub fn index(self) -> u8 {
        Self::ALL.iter().position(|c| *c == self).unwrap_or(0) as u8
    }

    /// Inverse of [`index`](GroupColor::index). Out-of-range indices clamp to
    /// [`GroupColor::Grey`] so a corrupt persisted value never panics.
    #[must_use]
    pub fn from_index(i: u8) -> GroupColor {
        Self::ALL.get(i as usize).copied().unwrap_or(GroupColor::Grey)
    }
}

impl Default for GroupColor {
    /// Default group colour is [`GroupColor::Grey`].
    fn default() -> Self {
        GroupColor::Grey
    }
}

/// A named, colour-coded group of tabs.
///
/// Tabs reference a group by its [`TabGroup::id`] via `TabEntry::group_id`.
/// The group itself carries only presentation state (label, colour, collapse
/// flag); membership lives on the tabs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabGroup {
    /// Stable id, unique within a session and never reused.
    pub id: usize,
    /// User-visible label rendered on the group's strip chip (may be empty).
    pub label: String,
    /// Colour for the strip chip and member tabs' accent bars.
    pub color: GroupColor,
    /// When `true`, every member tab except the leftmost is hidden from the
    /// strip; the leftmost member acts as the collapsed-group chip.
    pub collapsed: bool,
}

impl TabGroup {
    /// Create an expanded group with the given id, label and colour.
    #[must_use]
    pub fn new(id: usize, label: impl Into<String>, color: GroupColor) -> Self {
        Self { id, label: label.into(), color, collapsed: false }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_color_is_grey() {
        assert_eq!(GroupColor::default(), GroupColor::Grey);
    }

    #[test]
    fn color_blue_rgb() {
        let c = GroupColor::Blue.color();
        assert_eq!((c.r, c.g, c.b, c.a), (0x3B, 0x82, 0xF6, 255));
    }

    #[test]
    fn index_round_trips_for_every_variant() {
        for (i, c) in GroupColor::ALL.iter().enumerate() {
            assert_eq!(c.index() as usize, i);
            assert_eq!(GroupColor::from_index(i as u8), *c);
        }
    }

    #[test]
    fn from_index_out_of_range_is_grey() {
        assert_eq!(GroupColor::from_index(200), GroupColor::Grey);
    }

    #[test]
    fn new_group_is_expanded() {
        let g = TabGroup::new(7, "Research", GroupColor::Purple);
        assert_eq!(g.id, 7);
        assert_eq!(g.label, "Research");
        assert_eq!(g.color, GroupColor::Purple);
        assert!(!g.collapsed);
    }

    #[test]
    fn color_is_copy_and_hashable() {
        use std::collections::HashMap;
        let a = GroupColor::Green;
        let b = a; // Copy
        assert_eq!(a, b);
        let mut m: HashMap<GroupColor, u8> = HashMap::new();
        m.insert(GroupColor::Pink, 1);
        assert_eq!(m.get(&GroupColor::Pink), Some(&1));
    }
}
