//! Unit tests for the pure stack-shuffling core of `Lumen::navigate_by`
//! (`NavEntry::shift_history_entry`). The full `navigate_by` needs the live app
//! (window + renderer), so only the rendering-free hop is unit-tested here; the
//! destination popstate/reload is covered by `navigate_back`/`navigate_forward`.

use super::*;

/// Build a same-document `NavEntry` tagged by `tag` (used as the display URL
/// and the serialized state) so a hop's effect is observable by tag.
fn entry(tag: &str) -> NavEntry {
    NavEntry {
        source: PageSource::Static {
            html: String::new(),
            url: tag.to_string(),
        },
        scroll_x: 0.0,
        scroll_y: 0.0,
        display_url: Some(tag.to_string()),
        same_doc_state_json: Some(format!("\"{tag}\"")),
        nav_key: format!("nav-{tag}"),
        frame_target: None,
    }
}

/// Build a full-document `NavEntry` tagged by `tag` (`same_doc_state_json:
/// None`) — a genuine page-load boundary, as opposed to [`entry`]'s
/// same-document `pushState` entries.
fn full_entry(tag: &str) -> NavEntry {
    NavEntry {
        same_doc_state_json: None,
        ..entry(tag)
    }
}

#[test]
fn shift_back_once() {
    let mut nav_back = vec![entry("e1"), entry("e2")];
    let mut nav_fwd = vec![];
    let cur = entry("e3");

    let result = NavEntry::shift_history_entry(&mut nav_back, &mut nav_fwd, cur, true);

    assert_eq!(result.display_url, Some("e2".to_string()));
    assert_eq!(nav_back.len(), 1);
    assert_eq!(nav_back[0].display_url, Some("e1".to_string()));
    assert_eq!(nav_fwd.len(), 1);
    assert_eq!(nav_fwd[0].display_url, Some("e3".to_string()));
}

#[test]
fn shift_back_twice() {
    let mut nav_back = vec![entry("e1"), entry("e2")];
    let mut nav_fwd = vec![];
    let cur = entry("e3");

    let cur2 = NavEntry::shift_history_entry(&mut nav_back, &mut nav_fwd, cur, true);
    let cur3 = NavEntry::shift_history_entry(&mut nav_back, &mut nav_fwd, cur2, true);

    assert_eq!(cur3.display_url, Some("e1".to_string()));
    assert!(nav_back.is_empty());
    assert_eq!(nav_fwd.len(), 2);
    assert_eq!(nav_fwd[0].display_url, Some("e3".to_string()));
    assert_eq!(nav_fwd[1].display_url, Some("e2".to_string()));
}

#[test]
fn shift_forward_once() {
    let mut nav_fwd = vec![entry("f1")];
    let mut nav_back = vec![];
    let cur = entry("c");

    let result = NavEntry::shift_history_entry(&mut nav_back, &mut nav_fwd, cur, false);

    assert_eq!(result.display_url, Some("f1".to_string()));
    assert_eq!(nav_back.len(), 1);
    assert_eq!(nav_back[0].display_url, Some("c".to_string()));
    assert!(nav_fwd.is_empty());
}

#[test]
fn shift_multi_step_all_same_document_does_not_cross() {
    // steps=2 → 1 hop: pops the top of `nav_back` ("e2") and stops there
    // without ever reaching a full-document entry.
    let mut nav_back = vec![entry("e1"), entry("e2")];
    let mut nav_fwd = vec![];
    let cur = entry("cur");

    let (result, crossed) =
        NavEntry::shift_multi_step(&mut nav_back, &mut nav_fwd, cur, 2, true);

    assert_eq!(result.display_url, Some("e2".to_string()));
    assert!(!crossed);
}

#[test]
fn shift_multi_step_through_full_document_crosses() {
    // dest (same-doc, belongs to `full`) ← full (full-doc) ← mid1 ← mid2
    // ← cur. steps=4 → 3 hops: pops mid2, mid1 (both same-doc, no cross),
    // then `full` (full-doc) — the loaded document is now stale for
    // `dest`, the entry left on top of `nav_back` for the caller's own
    // (real destination) pop.
    let mut nav_back = vec![
        entry("dest"),
        full_entry("full"),
        entry("mid1"),
        entry("mid2"),
    ];
    let mut nav_fwd = vec![];
    let cur = entry("cur");

    let (result, crossed) =
        NavEntry::shift_multi_step(&mut nav_back, &mut nav_fwd, cur, 4, true);

    assert_eq!(result.display_url, Some("full".to_string()));
    assert!(crossed);
    assert_eq!(nav_back.len(), 1);
    assert_eq!(nav_back[0].display_url, Some("dest".to_string()));
}

#[test]
fn key_delta_found_in_back() {
    let nav_back = vec![entry("a"), entry("b")];
    let nav_fwd: Vec<NavEntry> = vec![];

    assert_eq!(Lumen::key_traversal_delta(&nav_back, &nav_fwd, "nav-a"), Some(-2));
    assert_eq!(Lumen::key_traversal_delta(&nav_back, &nav_fwd, "nav-b"), Some(-1));
}

#[test]
fn key_delta_found_in_fwd() {
    let nav_back: Vec<NavEntry> = vec![];
    let nav_fwd = vec![entry("x"), entry("y")];

    assert_eq!(Lumen::key_traversal_delta(&nav_back, &nav_fwd, "nav-x"), Some(2));
    assert_eq!(Lumen::key_traversal_delta(&nav_back, &nav_fwd, "nav-y"), Some(1));
}

#[test]
fn key_delta_missing_is_none() {
    let nav_back = vec![entry("a")];
    let nav_fwd = vec![entry("b")];

    assert_eq!(Lumen::key_traversal_delta(&nav_back, &nav_fwd, "nav-zzz"), None);
}
