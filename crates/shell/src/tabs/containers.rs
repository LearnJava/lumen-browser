//! Tab containers (7D.2): per-tab isolation for cookies/storage and a
//! coloured strip in the tab UI.
//!
//! A *container* is a logical bucket that scopes a tab's cookie jar, local
//! storage, IndexedDB and Service Worker registrations. Two tabs visiting
//! the same origin but in different containers see independent state — the
//! classic use case is signing into two accounts of the same site without
//! profile switching.
//!
//! At UI level, a non-`None` container draws a 3 px coloured border-top
//! strip on the tab button so the user can tell containers apart at a
//! glance. The colour comes from [`ContainerKind::border_color`].
//!
//! This module ships the data types ([`ContainerKind`], [`ContainerStore`])
//! and pure helpers. Plumbing into `TabEntry` lives in
//! [`super::strip`]; wiring into the shell `Lumen` struct lives in
//! `crates/shell/src/main.rs`.

// 7D.2 ships the data model and pure helpers ahead of the user-visible
// command surface: the omnibox/context-menu wiring that calls
// `KeyCommand::SetTabContainer(...)` with a non-`None` variant lands in a
// follow-up. Tolerate dead-code warnings module-wide until then; cargo
// clippy with `-D warnings` would otherwise reject this commit even though
// every item is covered by unit tests.
#![allow(dead_code)]

use std::collections::HashMap;

use lumen_layout::Color;

/// Kind of tab container. Drives the border-top colour in the tab strip
/// and the isolation key for cookies/storage.
///
/// `None` means "no container" — the default, shared state.
///
/// Built-in kinds (`Personal`/`Work`/`Finance`/`Shopping`) have fixed
/// brand colours chosen for high contrast against the dark tab strip.
/// `Custom(r, g, b)` lets the user pick an arbitrary RGB colour for
/// user-defined containers.
///
/// Derive `Copy` so the type can flow through `KeyCommand` and other
/// `Copy`-typed shell command enums without boxing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContainerKind {
    /// No container — default shared cookie/storage state.
    None,
    /// Personal container — violet (#7B61FF). Conventional "private life" bucket.
    Personal,
    /// Work container — blue (#3B82F6). Office accounts, business sites.
    Work,
    /// Finance container — green (#22C55E). Banking, payments, brokerage.
    Finance,
    /// Shopping container — orange (#F97316). E-commerce, marketplaces.
    Shopping,
    /// User-defined container with an arbitrary RGB border colour.
    Custom(u8, u8, u8),
}

impl ContainerKind {
    /// Border-top strip colour, or `None` for [`ContainerKind::None`].
    ///
    /// Returned colour is fully opaque (`a = 255`). Caller draws a thin
    /// `FillRect` at the top edge of the tab button.
    #[must_use]
    pub fn border_color(self) -> Option<Color> {
        match self {
            ContainerKind::None => None,
            ContainerKind::Personal => Some(Color { r: 0x7B, g: 0x61, b: 0xFF, a: 255 }),
            ContainerKind::Work => Some(Color { r: 0x3B, g: 0x82, b: 0xF6, a: 255 }),
            ContainerKind::Finance => Some(Color { r: 0x22, g: 0xC5, b: 0x5E, a: 255 }),
            ContainerKind::Shopping => Some(Color { r: 0xF9, g: 0x73, b: 0x16, a: 255 }),
            ContainerKind::Custom(r, g, b) => Some(Color { r, g, b, a: 255 }),
        }
    }

    /// Human-readable container name for UI labels.
    ///
    /// Returns `"None"` for [`ContainerKind::None`] and `"Custom"` for
    /// every [`ContainerKind::Custom`] regardless of its RGB tuple — the
    /// per-instance label is the user's responsibility to maintain.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            ContainerKind::None => "None",
            ContainerKind::Personal => "Personal",
            ContainerKind::Work => "Work",
            ContainerKind::Finance => "Finance",
            ContainerKind::Shopping => "Shopping",
            ContainerKind::Custom(_, _, _) => "Custom",
        }
    }
}

impl Default for ContainerKind {
    /// Default container kind is [`ContainerKind::None`].
    fn default() -> Self {
        ContainerKind::None
    }
}

/// Origin+container → cookie/storage store id.
///
/// Keying scheme: a tuple of `(origin, ContainerKind)` maps to an opaque
/// `u32` store id. The store id is what cookie jars and storage backends
/// will eventually use to partition their data — for now it is just a
/// stable identifier handed out monotonically.
///
/// The actual isolation pipeline (cookie jar lookup, storage backend
/// dispatch) is wired in later tasks; `ContainerStore` only owns the
/// mapping.
#[derive(Debug, Default)]
pub struct ContainerStore {
    /// `(origin, container) → store id`.
    map: HashMap<(String, ContainerKind), u32>,
    /// Monotonic counter for fresh store ids; never reused within a session.
    next_id: u32,
}

impl ContainerStore {
    /// Create an empty store. First minted id will be `0`.
    #[must_use]
    pub fn new() -> Self {
        Self { map: HashMap::new(), next_id: 0 }
    }

    /// Get the store id for `(origin, container)`, allocating a fresh one
    /// on first lookup.
    ///
    /// Stable across calls within a session: the same `(origin, container)`
    /// pair always returns the same id.
    pub fn get_or_create(&mut self, origin: &str, container: ContainerKind) -> u32 {
        let key = (origin.to_owned(), container);
        if let Some(id) = self.map.get(&key) {
            return *id;
        }
        let id = self.next_id;
        self.next_id += 1;
        self.map.insert(key, id);
        id
    }

    /// Look up an existing store id without allocating.
    #[must_use]
    pub fn get(&self, origin: &str, container: ContainerKind) -> Option<u32> {
        self.map.get(&(origin.to_owned(), container)).copied()
    }

    /// Number of `(origin, container)` mappings tracked.
    #[must_use]
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// `true` if no mapping has been allocated yet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn border_color_none_returns_none() {
        assert!(ContainerKind::None.border_color().is_none());
    }

    #[test]
    fn border_color_personal_is_violet() {
        let c = ContainerKind::Personal.border_color().expect("Personal has colour");
        assert_eq!((c.r, c.g, c.b, c.a), (0x7B, 0x61, 0xFF, 255));
    }

    #[test]
    fn border_color_work_is_blue() {
        let c = ContainerKind::Work.border_color().expect("Work has colour");
        assert_eq!((c.r, c.g, c.b, c.a), (0x3B, 0x82, 0xF6, 255));
    }

    #[test]
    fn border_color_finance_is_green() {
        let c = ContainerKind::Finance.border_color().expect("Finance has colour");
        assert_eq!((c.r, c.g, c.b, c.a), (0x22, 0xC5, 0x5E, 255));
    }

    #[test]
    fn border_color_shopping_is_orange() {
        let c = ContainerKind::Shopping.border_color().expect("Shopping has colour");
        assert_eq!((c.r, c.g, c.b, c.a), (0xF9, 0x73, 0x16, 255));
    }

    #[test]
    fn border_color_custom_uses_given_rgb() {
        let c = ContainerKind::Custom(10, 20, 30).border_color().expect("Custom has colour");
        assert_eq!((c.r, c.g, c.b, c.a), (10, 20, 30, 255));
    }

    #[test]
    fn name_for_each_variant() {
        assert_eq!(ContainerKind::None.name(), "None");
        assert_eq!(ContainerKind::Personal.name(), "Personal");
        assert_eq!(ContainerKind::Work.name(), "Work");
        assert_eq!(ContainerKind::Finance.name(), "Finance");
        assert_eq!(ContainerKind::Shopping.name(), "Shopping");
        assert_eq!(ContainerKind::Custom(0, 0, 0).name(), "Custom");
    }

    #[test]
    fn default_is_none() {
        assert_eq!(ContainerKind::default(), ContainerKind::None);
    }

    #[test]
    fn kind_is_copy_and_eq() {
        // Compile-time-ish check: type implements Copy + Eq + Hash for KeyCommand wiring.
        let a = ContainerKind::Work;
        let b = a; // Copy
        assert_eq!(a, b);
        let mut map: HashMap<ContainerKind, u32> = HashMap::new();
        map.insert(ContainerKind::Personal, 1);
        assert_eq!(map.get(&ContainerKind::Personal), Some(&1));
    }

    #[test]
    fn store_new_is_empty() {
        let s = ContainerStore::new();
        assert!(s.is_empty());
        assert_eq!(s.len(), 0);
    }

    #[test]
    fn store_get_or_create_mints_fresh_ids() {
        let mut s = ContainerStore::new();
        let id_a = s.get_or_create("https://example.com", ContainerKind::Work);
        let id_b = s.get_or_create("https://example.com", ContainerKind::Personal);
        let id_c = s.get_or_create("https://other.com", ContainerKind::Work);
        assert_ne!(id_a, id_b);
        assert_ne!(id_a, id_c);
        assert_ne!(id_b, id_c);
        assert_eq!(s.len(), 3);
    }

    #[test]
    fn store_get_or_create_is_stable() {
        let mut s = ContainerStore::new();
        let first = s.get_or_create("https://example.com", ContainerKind::Work);
        let second = s.get_or_create("https://example.com", ContainerKind::Work);
        assert_eq!(first, second);
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn store_isolates_origin_by_container() {
        let mut s = ContainerStore::new();
        let work = s.get_or_create("https://bank.example", ContainerKind::Work);
        let finance = s.get_or_create("https://bank.example", ContainerKind::Finance);
        assert_ne!(work, finance, "same origin + different container must isolate");
    }

    #[test]
    fn store_get_returns_some_after_create() {
        let mut s = ContainerStore::new();
        let id = s.get_or_create("https://example.com", ContainerKind::Personal);
        assert_eq!(s.get("https://example.com", ContainerKind::Personal), Some(id));
    }

    #[test]
    fn store_get_returns_none_when_missing() {
        let s = ContainerStore::new();
        assert!(s.get("https://example.com", ContainerKind::Personal).is_none());
    }

    #[test]
    fn store_custom_variants_with_same_rgb_share_bucket() {
        let mut s = ContainerStore::new();
        let a = s.get_or_create("https://x.test", ContainerKind::Custom(1, 2, 3));
        let b = s.get_or_create("https://x.test", ContainerKind::Custom(1, 2, 3));
        assert_eq!(a, b, "same Custom(r,g,b) must hash to the same key");
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn store_custom_variants_with_diff_rgb_isolate() {
        let mut s = ContainerStore::new();
        let a = s.get_or_create("https://x.test", ContainerKind::Custom(1, 2, 3));
        let b = s.get_or_create("https://x.test", ContainerKind::Custom(9, 9, 9));
        assert_ne!(a, b);
        assert_eq!(s.len(), 2);
    }

    #[test]
    fn store_first_id_is_zero() {
        let mut s = ContainerStore::new();
        let id = s.get_or_create("https://example.com", ContainerKind::None);
        assert_eq!(id, 0);
    }
}
