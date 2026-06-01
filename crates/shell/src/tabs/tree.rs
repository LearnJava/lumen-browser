//! Tree-style tab utilities (7A.2): depth, children, subtree, visible order.
//!
//! `TabEntry::opener_id` forms a forest (set of trees). Each root tab has
//! `opener_id = None`; child tabs point at their parent's `id`.
//!
//! All functions operate on `&[TabEntry]` slices so they are pure and easy
//! to test without constructing a full `TabStrip`.

use super::strip::TabEntry;

/// Maximum tree depth accepted by depth_of.
///
/// Caps iteration to avoid O(n²) worst case on pathological data; real
/// browser sessions stay well under 64 levels.
const MAX_DEPTH: usize = 64;

/// Compute the tree depth of the tab with `id` in the given slice.
///
/// Depth 0 = root tab (`opener_id = None`).
/// Depth 1 = direct child of a root tab, etc.
/// Returns 0 for unknown `id` (graceful degradation).
pub fn depth_of(tabs: &[TabEntry], id: usize) -> usize {
    let mut current_id = id;
    for depth in 0..MAX_DEPTH {
        let entry = tabs.iter().find(|t| t.id == current_id);
        match entry {
            None => return depth,
            Some(tab) => match tab.opener_id {
                None => return depth,
                Some(parent_id) => current_id = parent_id,
            },
        }
    }
    MAX_DEPTH
}

/// Return the IDs of direct children of `parent_id` in strip order.
pub fn children_of(tabs: &[TabEntry], parent_id: usize) -> Vec<usize> {
    tabs.iter()
        .filter(|t| t.opener_id == Some(parent_id))
        .map(|t| t.id)
        .collect()
}

/// Collect the IDs of all tabs in the subtree rooted at `root_id` (inclusive).
///
/// Order follows the strip (insertion) order.
pub fn subtree_ids(tabs: &[TabEntry], root_id: usize) -> Vec<usize> {
    let mut result = Vec::new();
    collect_subtree(tabs, root_id, &mut result);
    result
}

fn collect_subtree(tabs: &[TabEntry], root_id: usize, out: &mut Vec<usize>) {
    out.push(root_id);
    for tab in tabs.iter().filter(|t| t.opener_id == Some(root_id)) {
        collect_subtree(tabs, tab.id, out);
    }
}

/// A row item produced by [`visible_order`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisibleRow {
    /// Index of the tab entry in `TabStrip::tabs`.
    pub strip_idx: usize,
    /// Stable tab identifier.
    pub id: usize,
    /// Visual indentation depth (0 = root, 1 = child, …).
    pub depth: usize,
    /// `true` when this tab has at least one child in the strip.
    pub has_children: bool,
}

/// Build the ordered list of visible tabs for tree-style rendering.
///
/// Iterates `tabs` in strip order; tabs whose ancestor is present in
/// `collapsed` are skipped. A tab is collapsed when its *own* ID is in
/// `collapsed` (children hidden), not when its parent is collapsed —
/// the tab itself is still visible but its subtree is not.
///
/// `collapsed` contains the IDs of tabs whose subtrees are currently hidden.
pub fn visible_order(tabs: &[TabEntry], collapsed: &std::collections::HashSet<usize>) -> Vec<VisibleRow> {
    let mut rows = Vec::with_capacity(tabs.len());
    for (idx, tab) in tabs.iter().enumerate() {
        if is_ancestor_collapsed(tabs, tab.id, collapsed) {
            continue;
        }
        let depth = depth_of(tabs, tab.id);
        let has_children = !children_of(tabs, tab.id).is_empty();
        rows.push(VisibleRow {
            strip_idx: idx,
            id: tab.id,
            depth,
            has_children,
        });
    }
    rows
}

/// Returns `true` if any *ancestor* (not self) of `id` is in `collapsed`.
fn is_ancestor_collapsed(tabs: &[TabEntry], id: usize, collapsed: &std::collections::HashSet<usize>) -> bool {
    let entry = match tabs.iter().find(|t| t.id == id) {
        Some(e) => e,
        None => return false,
    };
    let parent_id = match entry.opener_id {
        Some(pid) => pid,
        None => return false,
    };
    if collapsed.contains(&parent_id) {
        return true;
    }
    is_ancestor_collapsed(tabs, parent_id, collapsed)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tab_lifecycle::state::TabState;

    fn make_tab(id: usize, opener: Option<usize>) -> TabEntry {
        TabEntry {
            id,
            title: format!("tab-{id}"),
            tab_state: TabState::Active,
            opener_id: opener,
        }
    }

    /// Build a tree:
    ///   0 (root)
    ///   ├─ 1
    ///   │  └─ 3
    ///   └─ 2
    ///   4 (root)
    fn sample_tabs() -> Vec<TabEntry> {
        vec![
            make_tab(0, None),
            make_tab(1, Some(0)),
            make_tab(2, Some(0)),
            make_tab(3, Some(1)),
            make_tab(4, None),
        ]
    }

    #[test]
    fn depth_of_root_is_zero() {
        let tabs = sample_tabs();
        assert_eq!(depth_of(&tabs, 0), 0);
        assert_eq!(depth_of(&tabs, 4), 0);
    }

    #[test]
    fn depth_of_child_is_one() {
        let tabs = sample_tabs();
        assert_eq!(depth_of(&tabs, 1), 1);
        assert_eq!(depth_of(&tabs, 2), 1);
    }

    #[test]
    fn depth_of_grandchild_is_two() {
        let tabs = sample_tabs();
        assert_eq!(depth_of(&tabs, 3), 2);
    }

    #[test]
    fn depth_of_unknown_id_returns_zero() {
        let tabs = sample_tabs();
        assert_eq!(depth_of(&tabs, 99), 0);
    }

    #[test]
    fn children_of_root() {
        let tabs = sample_tabs();
        let ch = children_of(&tabs, 0);
        assert_eq!(ch, vec![1, 2]);
    }

    #[test]
    fn children_of_leaf_is_empty() {
        let tabs = sample_tabs();
        assert!(children_of(&tabs, 3).is_empty());
    }

    #[test]
    fn subtree_ids_includes_all_descendants() {
        let tabs = sample_tabs();
        let sub = subtree_ids(&tabs, 0);
        assert!(sub.contains(&0));
        assert!(sub.contains(&1));
        assert!(sub.contains(&2));
        assert!(sub.contains(&3));
        assert!(!sub.contains(&4));
    }

    #[test]
    fn subtree_ids_leaf() {
        let tabs = sample_tabs();
        assert_eq!(subtree_ids(&tabs, 3), vec![3]);
    }

    #[test]
    fn visible_order_no_collapsed() {
        let tabs = sample_tabs();
        let collapsed = std::collections::HashSet::new();
        let rows = visible_order(&tabs, &collapsed);
        assert_eq!(rows.len(), 5);
        assert_eq!(rows[0].id, 0);
        assert_eq!(rows[0].depth, 0);
        assert!(rows[0].has_children);
        assert_eq!(rows[3].id, 3);
        assert_eq!(rows[3].depth, 2);
        assert!(!rows[3].has_children);
    }

    #[test]
    fn visible_order_collapse_hides_children() {
        let tabs = sample_tabs();
        let mut collapsed = std::collections::HashSet::new();
        collapsed.insert(0); // collapse tab 0 → hide 1, 2, 3
        let rows = visible_order(&tabs, &collapsed);
        // Only tab 0 and tab 4 should be visible.
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].id, 0);
        assert_eq!(rows[1].id, 4);
    }

    #[test]
    fn visible_order_collapse_intermediate() {
        let tabs = sample_tabs();
        let mut collapsed = std::collections::HashSet::new();
        collapsed.insert(1); // collapse tab 1 → hide only tab 3
        let rows = visible_order(&tabs, &collapsed);
        let ids: Vec<usize> = rows.iter().map(|r| r.id).collect();
        assert_eq!(ids, vec![0, 1, 2, 4]);
    }

    #[test]
    fn visible_order_strip_idx_matches_position() {
        let tabs = sample_tabs();
        let collapsed = std::collections::HashSet::new();
        let rows = visible_order(&tabs, &collapsed);
        for row in &rows {
            assert_eq!(tabs[row.strip_idx].id, row.id);
        }
    }
}
