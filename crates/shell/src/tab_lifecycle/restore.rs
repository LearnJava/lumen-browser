//! T3 hibernation and restore helpers (ADR-008 §10J).
//!
//! When a background tab ages from T2 (BackgroundOld) to T3 (Hibernated), the
//! shell calls [`TabMetadata`] and saves the full DOM snapshot to SQLite via
//! [`lumen_storage::TabSnapshotStore`].  On the next switch to the hibernated
//! tab, the shell calls the restore path — `Document::from_bytes()` → re-parse
//! CSS → relayout+paint → swap into the active slot.
//!
//! Goal SLOs:
//!  - Hibernate:  ≤ 50 ms  (serialise + write; background thread candidate)
//!  - Restore:    ≤ 1 500 ms  (deserialise + CSS parse + full layout+paint)

/// Lightweight per-tab identity kept in RAM while a tab is hibernated (T3).
///
/// At most ~200 bytes per entry — 50 hibernated tabs cost < 10 KB total.
/// Keyed by `TabEntry::id` in `Lumen::hibernated_tabs`.
///
/// Fields are used to display tab info instantly (strip title, address bar)
/// before the SQLite restore completes.  Scroll state is not kept here because
/// it is only needed at restore time and is already stored in SQLite.
#[derive(Debug, Clone)]
pub struct TabMetadata {
    /// Original page URL — shown in the address bar while the tab is hibernated.
    pub url: String,
    /// Tab title at the time of hibernation — shown in the tab strip.
    pub title: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_metadata_fields() {
        let m = TabMetadata {
            url: "https://example.com/".into(),
            title: "Example".into(),
        };
        assert_eq!(m.url, "https://example.com/");
        assert_eq!(m.title, "Example");
    }

    #[test]
    fn tab_metadata_clone() {
        let m = TabMetadata {
            url: "https://rust-lang.org/".into(),
            title: "Rust".into(),
        };
        let c = m.clone();
        assert_eq!(c.url, m.url);
        assert_eq!(c.title, m.title);
    }
}
