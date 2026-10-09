//! Stack of recently closed tabs (UX-REOPEN-TAB): Ctrl+Shift+T and the tab
//! context-menu row pop the newest entry and reopen it.
//!
//! Only what is needed to navigate back is kept (URL, title, scroll offset,
//! pinned flag, container, group id) — the page itself is not preserved.

use super::containers::ContainerKind;

/// Maximum number of remembered closed tabs; the oldest is dropped first.
pub const MAX_CLOSED: usize = 25;

/// One closed tab, as needed to reopen it.
#[derive(Debug, Clone, PartialEq)]
pub struct ClosedTab {
    /// Page URL (never empty — blank tabs are not remembered).
    pub url: String,
    /// Tab title at the time of closing.
    pub title: String,
    /// Horizontal scroll offset, CSS px.
    pub scroll_x: f32,
    /// Vertical scroll offset, CSS px.
    pub scroll_y: f32,
    /// Whether the tab was pinned.
    pub pinned: bool,
    /// Container the tab belonged to.
    pub container: ContainerKind,
    /// Tab-group id, restored only if the group still exists.
    pub group_id: Option<usize>,
}

/// LIFO stack of closed tabs.
#[derive(Debug, Default)]
pub struct ClosedTabs {
    stack: Vec<ClosedTab>,
}

impl ClosedTabs {
    /// Remember a closed tab. Entries with an empty URL are ignored.
    pub fn push(&mut self, tab: ClosedTab) {
        if tab.url.is_empty() {
            return;
        }
        if self.stack.len() == MAX_CLOSED {
            self.stack.remove(0);
        }
        self.stack.push(tab);
    }

    /// Take the most recently closed tab.
    pub fn pop(&mut self) -> Option<ClosedTab> {
        self.stack.pop()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(url: &str) -> ClosedTab {
        ClosedTab {
            url: url.to_owned(),
            title: String::new(),
            scroll_x: 0.0,
            scroll_y: 120.0,
            pinned: false,
            container: ContainerKind::None,
            group_id: None,
        }
    }

    #[test]
    fn pops_newest_first() {
        let mut s = ClosedTabs::default();
        s.push(tab("https://a/"));
        s.push(tab("https://b/"));
        assert_eq!(s.pop().unwrap().url, "https://b/");
        assert_eq!(s.pop().unwrap().url, "https://a/");
        assert!(s.pop().is_none());
    }

    #[test]
    fn empty_url_is_not_remembered() {
        let mut s = ClosedTabs::default();
        s.push(tab(""));
        assert!(s.pop().is_none());
    }

    #[test]
    fn capped_drops_oldest() {
        let mut s = ClosedTabs::default();
        for i in 0..MAX_CLOSED + 3 {
            s.push(tab(&format!("https://x/{i}")));
        }
        assert_eq!(s.pop().unwrap().url, format!("https://x/{}", MAX_CLOSED + 2));
        let mut n = 1;
        let mut last = String::new();
        while let Some(t) = s.pop() {
            n += 1;
            last = t.url;
        }
        assert_eq!(n, MAX_CLOSED);
        assert_eq!(last, "https://x/3");
    }
}
