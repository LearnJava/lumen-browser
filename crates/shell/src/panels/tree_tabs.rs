//! Tree-style tabs state (7A.2): which subtrees of the opener tree are
//! folded. The rows themselves — indentation by depth, the ▾/▸ collapse arrow
//! on a parent — are drawn by the engine chrome's sidebar list (`#sbTabs`,
//! `ChromeTabModel::tree`, UX-CHROME-PANELS 14); `chrome_model_snapshot`
//! feeds it `tabs::tree::visible_order`, and the arrow's
//! `toggle-tab-subtree` action flips [`TreeTabsPanel::toggle_collapsed`].

use std::collections::HashSet;

/// Tree-style tabs state.
pub struct TreeTabsPanel {
    /// `true` while the tree layout is on (`Ctrl+Shift+B`). Implies the
    /// vertical sidebar layout.
    pub visible: bool,
    /// Set of tab IDs whose direct children are hidden.
    pub collapsed: HashSet<usize>,
}

impl TreeTabsPanel {
    /// Create a new hidden state with no collapsed subtrees.
    pub fn new() -> Self {
        Self { visible: false, collapsed: HashSet::new() }
    }

    /// Flip visibility. Caller must trigger relayout + redraw.
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }

    /// Toggle the collapsed state of the subtree rooted at `tab_id`.
    ///
    /// If the subtree is currently expanded, it becomes collapsed (children
    /// hidden). If collapsed, it becomes expanded.
    pub fn toggle_collapsed(&mut self, tab_id: usize) {
        if !self.collapsed.remove(&tab_id) {
            self.collapsed.insert(tab_id);
        }
    }
}

impl Default for TreeTabsPanel {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_panel_is_hidden() {
        assert!(!TreeTabsPanel::new().visible);
    }

    #[test]
    fn toggle_panel_visibility() {
        let mut p = TreeTabsPanel::new();
        p.toggle();
        assert!(p.visible);
        p.toggle();
        assert!(!p.visible);
    }

    #[test]
    fn toggle_collapsed_adds_and_removes() {
        let mut p = TreeTabsPanel::new();
        p.toggle_collapsed(7);
        assert!(p.collapsed.contains(&7));
        p.toggle_collapsed(7);
        assert!(!p.collapsed.contains(&7));
    }
}
