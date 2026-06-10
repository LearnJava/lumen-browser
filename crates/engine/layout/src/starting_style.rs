//! `@starting-style` algorithm stub — CSS Transitions Level 2 §3.4.
//!
//! `@starting-style` provides the "before-change style" for CSS transitions
//! when an element *enters* the document for the first time or becomes visible
//! after `display: none`. This enables "enter animations":
//!
//! ```css
//! @starting-style {
//!     dialog { opacity: 0; transform: scale(0.9); }
//! }
//! dialog {
//!     opacity: 1;
//!     transform: scale(1);
//!     transition: opacity 0.3s, transform 0.3s;
//! }
//! ```
//!
//! When `<dialog>` is shown for the first time the transition engine uses
//! `opacity: 0; transform: scale(0.9)` as the *from* state instead of the
//! element's prior computed style (which didn't exist).
//!
//! ## P1/P4 split
//!
//! **P1 (this module)** — data structures and selector lookup:
//! - [`StartingStyleTracker`] — tracks which nodes are "entering"
//! - [`resolve_starting_style`] — matches `@starting-style` rules against a node
//!
//! **P4 (CSS: @starting-style)** — cascade integration:
//! - Call `tracker.mark_entered(node)` whenever a node is inserted into the
//!   DOM or its computed `display` transitions from `none` → non-`none`.
//! - In `TransitionScheduler::sync`, before building `from_val`:
//!   if `tracker.is_entered(node)` → call `resolve_starting_style(…)`,
//!   compute a `ComputedStyle` from the returned declarations (via
//!   `apply_declaration` on a fresh `ComputedStyle::default()`), then use that
//!   as the *before-change* style. After transitions are started, call
//!   `tracker.consume(node)`.
//! - See `// CSS: @starting-style` comment in `animation.rs`.

use std::collections::HashSet;

use lumen_css_parser::{Declaration, Rule, Stylesheet};
use lumen_dom::{Document, NodeId};

use crate::style::matches_complex;

// ─── StartingStyleTracker ────────────────────────────────────────────────────

/// Tracks nodes that are "entering" — i.e. have just been inserted into the
/// document or changed `display` from `none` to a visible value.
///
/// An "entered" node uses `@starting-style` rules as its before-change style
/// for transition purposes (CSS Transitions L2 §3.4). Once the transition
/// scheduler has consumed the entry state, the node is removed from the set
/// so subsequent relayouts do not re-apply the starting style.
#[derive(Debug, Default, Clone)]
pub struct StartingStyleTracker {
    /// Nodes currently in the "entering" state.
    entered: HashSet<NodeId>,
}

impl StartingStyleTracker {
    /// Create an empty tracker.
    pub fn new() -> Self {
        Self::default()
    }

    /// Mark `node` as "just entered" the document (or became visible).
    ///
    /// Call this when:
    /// - A node is inserted into the DOM and has at least one CSS transition.
    /// - A node's computed `display` changes from `none` to a non-`none` value.
    ///
    /// The node stays in the "entered" set until [`Self::consume`] is called
    /// (which happens in `TransitionScheduler::sync` once the entry transition
    /// starts). Duplicate calls for the same node are harmless.
    pub fn mark_entered(&mut self, node: NodeId) {
        self.entered.insert(node);
    }

    /// Returns `true` when `node` was marked via [`Self::mark_entered`] and
    /// the entry transition has not yet been consumed.
    pub fn is_entered(&self, node: NodeId) -> bool {
        self.entered.contains(&node)
    }

    /// Remove `node` from the "entered" set.
    ///
    /// Called by P4's `TransitionScheduler::sync` after the starting style has
    /// been used as the `from` value for entry transitions. After this call
    /// the node is treated as a normal (non-entering) element.
    pub fn consume(&mut self, node: NodeId) {
        self.entered.remove(&node);
    }

    /// Remove all state for `node` — called when the node leaves the DOM.
    ///
    /// This prevents stale entries from affecting a node that is later
    /// reinserted at a different position.
    pub fn remove(&mut self, node: NodeId) {
        self.entered.remove(&node);
    }
}

// ─── resolve_starting_style ──────────────────────────────────────────────────

/// Look up `@starting-style` declarations that match `node` in `sheet`.
///
/// Iterates all `@starting-style` blocks in the stylesheet, then for each
/// contained rule tries to match its selectors against `node` using the same
/// `matches_complex` logic as the main cascade. Returns the combined
/// declaration list from all matching rules in source order (later rules
/// append to the list — caller is responsible for precedence/merge).
///
/// Returns `None` when `sheet.starting_style_rules` is empty or no rule
/// matches `node`.
///
/// # CSS: @starting-style
/// P4 integrates this in `TransitionScheduler::sync`:
/// ```text
/// if tracker.is_entered(node) {
///     if let Some(decls) = resolve_starting_style(node, doc, sheet) {
///         // build ComputedStyle from decls via apply_declaration,
///         // use it as before_style argument to TransitionScheduler::sync
///     }
///     tracker.consume(node);
/// }
/// ```
pub fn resolve_starting_style(
    node: NodeId,
    doc: &Document,
    sheet: &Stylesheet,
) -> Option<Vec<Declaration>> {
    if sheet.starting_style_rules.is_empty() {
        return None;
    }

    let mut matched: Vec<Declaration> = Vec::new();

    for ss_rule in &sheet.starting_style_rules {
        collect_matching_declarations(node, doc, &ss_rule.rules, &mut matched);
    }

    if matched.is_empty() { None } else { Some(matched) }
}

/// Walk `rules` and append declarations from rules whose selectors match `node`.
fn collect_matching_declarations(
    node: NodeId,
    doc: &Document,
    rules: &[Rule],
    out: &mut Vec<Declaration>,
) {
    for rule in rules {
        for selector in &rule.selectors {
            if matches_complex(selector, doc, node) {
                out.extend_from_slice(&rule.declarations);
                break; // one matching selector per rule is enough
            }
        }
    }
}

// ─── tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_dom::NodeId;

    fn make_node_id(n: usize) -> NodeId {
        NodeId::from_index(n)
    }

    // ── StartingStyleTracker ──

    #[test]
    fn tracker_mark_and_is_entered() {
        let mut t = StartingStyleTracker::new();
        let n = make_node_id(1);
        assert!(!t.is_entered(n));
        t.mark_entered(n);
        assert!(t.is_entered(n));
    }

    #[test]
    fn tracker_consume_clears_flag() {
        let mut t = StartingStyleTracker::new();
        let n = make_node_id(2);
        t.mark_entered(n);
        t.consume(n);
        assert!(!t.is_entered(n));
    }

    #[test]
    fn tracker_remove_clears_flag() {
        let mut t = StartingStyleTracker::new();
        let n = make_node_id(3);
        t.mark_entered(n);
        t.remove(n);
        assert!(!t.is_entered(n));
    }

    #[test]
    fn tracker_duplicate_mark_is_harmless() {
        let mut t = StartingStyleTracker::new();
        let n = make_node_id(4);
        t.mark_entered(n);
        t.mark_entered(n); // second call must not panic or reset state
        assert!(t.is_entered(n));
    }

    #[test]
    fn tracker_multiple_nodes_independent() {
        let mut t = StartingStyleTracker::new();
        let a = make_node_id(5);
        let b = make_node_id(6);
        t.mark_entered(a);
        assert!(t.is_entered(a));
        assert!(!t.is_entered(b));
        t.consume(a);
        assert!(!t.is_entered(a));
    }

    #[test]
    fn tracker_consume_nonexistent_is_harmless() {
        let mut t = StartingStyleTracker::new();
        // consume on a node that was never marked must not panic
        t.consume(make_node_id(99));
    }

    // ── resolve_starting_style ──

    /// Find the first element node at a given depth from `start` (depth=1 → start itself).
    fn first_element_at_depth(doc: &Document, start: NodeId, depth: usize) -> Option<NodeId> {
        use lumen_dom::NodeData;
        if depth == 1 {
            return match &doc.get(start).data {
                NodeData::Element { .. } => Some(start),
                _ => None,
            };
        }
        for &child in &doc.get(start).children {
            if let Some(found) = first_element_at_depth(doc, child, depth - 1) {
                return Some(found);
            }
        }
        None
    }

    #[test]
    fn resolve_returns_none_when_no_starting_style_rules() {
        let doc = lumen_html_parser::parse("<dialog></dialog>");
        let sheet = lumen_css_parser::parse("dialog { opacity: 1; }");
        assert!(sheet.starting_style_rules.is_empty());

        // html(1) > body(2) > dialog(3) — depth 3 from document root
        let root = doc.root();
        if let Some(node) = first_element_at_depth(&doc, root, 4) {
            assert!(resolve_starting_style(node, &doc, &sheet).is_none());
        }
    }

    #[test]
    fn resolve_returns_declarations_for_matching_selector() {
        let doc = lumen_html_parser::parse("<dialog></dialog>");
        let sheet = lumen_css_parser::parse(
            "@starting-style { dialog { opacity: 0; } }",
        );
        assert_eq!(sheet.starting_style_rules.len(), 1);

        let root = doc.root();
        // document(depth=1) > html(2) > body(3) > dialog(4)
        if let Some(node) = first_element_at_depth(&doc, root, 4) {
            let decls = resolve_starting_style(node, &doc, &sheet);
            assert!(decls.is_some(), "expected matching declarations");
            let decls = decls.unwrap();
            assert_eq!(decls.len(), 1);
            assert_eq!(decls[0].property, "opacity");
            assert_eq!(decls[0].value.trim(), "0");
        }
    }

    #[test]
    fn resolve_returns_none_for_non_matching_selector() {
        let doc = lumen_html_parser::parse("<p></p>");
        // @starting-style has rule for `dialog`, but document has `<p>`
        let sheet = lumen_css_parser::parse(
            "@starting-style { dialog { opacity: 0; } }",
        );

        let root = doc.root();
        if let Some(node) = first_element_at_depth(&doc, root, 4) {
            assert!(resolve_starting_style(node, &doc, &sheet).is_none());
        }
    }

    #[test]
    fn resolve_merges_declarations_from_multiple_blocks() {
        let doc = lumen_html_parser::parse("<dialog></dialog>");
        let sheet = lumen_css_parser::parse(
            "@starting-style { dialog { opacity: 0; } }
             @starting-style { dialog { color: red; } }",
        );
        assert_eq!(sheet.starting_style_rules.len(), 2);

        let root = doc.root();
        if let Some(node) = first_element_at_depth(&doc, root, 4) {
            let decls = resolve_starting_style(node, &doc, &sheet).unwrap();
            assert_eq!(decls.len(), 2, "both @starting-style blocks should contribute");
            assert!(decls.iter().any(|d| d.property == "opacity"));
            assert!(decls.iter().any(|d| d.property == "color"));
        }
    }

    #[test]
    fn resolve_merges_multiple_decls_in_one_rule() {
        let doc = lumen_html_parser::parse("<dialog></dialog>");
        let sheet = lumen_css_parser::parse(
            "@starting-style { dialog { opacity: 0; color: blue; } }",
        );

        let root = doc.root();
        if let Some(node) = first_element_at_depth(&doc, root, 4) {
            let decls = resolve_starting_style(node, &doc, &sheet).unwrap();
            assert_eq!(decls.len(), 2);
        }
    }
}
