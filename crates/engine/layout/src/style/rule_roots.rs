//! BUG-935 срез 64 — restyle roots for a change of the cascade sheet itself.
//!
//! [`lumen_css_parser::Stylesheet::changed_plain_rules`] names the plain style rules that
//! were added, removed or moved between two versions of the sheet. The elements whose
//! computed style can differ are exactly the ones those rules select: a rule's effect on
//! an element is decided by the element alone (and its ancestors/siblings for the
//! combinators), never by the style of another element. Each such element is returned as a
//! root whose whole subtree is restyled — what it passes down by inheritance changes too.

use std::collections::HashSet;

use lumen_css_parser::{ComplexSelector, PseudoClass, Rule, SimpleSelector};
use lumen_dom::{Document, NodeData, NodeId};

use crate::style::matches_complex;

/// More changed rules than this and the walk costs about what the full cascade does.
const MAX_RULES: usize = 128;

/// The deep restyle roots for the style rules in `changed`, matched against `doc`'s light
/// tree. `None` when the answer cannot be given by looking at the elements alone — the rule
/// has a pseudo-element or `:host` in it — or the change is too wide to be worth it; the
/// caller then recascades everything.
///
/// A pseudo-element rule (`a::before`, `p::first-line`) selects an element whose *own* style
/// may come out unchanged while the box tree under it does not (generated content, the first
/// line cut out of a run), and an incremental flush trusts an equal style to mean an
/// unchanged subtree. So such a rule is not expressible here, rather than matched by its
/// originating element.
///
/// No returned root is below another one — a subtree already restyled is not walked again.
#[must_use]
pub fn restyle_roots_for_rule_changes(doc: &Document, changed: &[&Rule]) -> Option<HashSet<NodeId>> {
    if changed.len() > MAX_RULES {
        return None;
    }
    let mut selectors: Vec<&ComplexSelector> = Vec::new();
    for rule in changed {
        for complex in &rule.selectors {
            if !selects_elements_only(complex) {
                return None;
            }
            selectors.push(complex);
        }
    }
    let mut roots = HashSet::new();
    if selectors.is_empty() {
        return Some(roots);
    }
    let mut stack = vec![doc.root()];
    while let Some(id) = stack.pop() {
        let node = doc.get(id);
        if matches!(node.data, NodeData::Element { .. }) && selectors.iter().any(|s| matches_complex(s, doc, id)) {
            roots.insert(id);
            continue;
        }
        stack.extend(node.children.iter().copied());
    }
    Some(roots)
}

/// `complex` has no pseudo-element and no `:host` in any compound.
fn selects_elements_only(complex: &ComplexSelector) -> bool {
    std::iter::once(&complex.head).chain(complex.tail.iter().map(|(_, c)| c)).all(|compound| {
        compound.parts.iter().all(|part| {
            !matches!(part, SimpleSelector::PseudoElement(_) | SimpleSelector::PseudoClass(PseudoClass::Host(_)))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_css_parser::parse as parse_css;
    use lumen_html_parser::parse as parse_html;

    const HTML: &str = "<html><body><div id=\"a\" class=\"c\"><p id=\"p1\"><b id=\"b1\">x</b></p></div>\
                        <div id=\"b\" class=\"c d\"><p id=\"p2\">y</p></div></body></html>";

    fn roots(css: &str) -> Option<Vec<String>> {
        let doc = parse_html(HTML);
        let sheet = parse_css(css);
        let rules: Vec<&Rule> = sheet.rules.iter().collect();
        let found = restyle_roots_for_rule_changes(&doc, &rules)?;
        let mut ids: Vec<String> = found
            .iter()
            .map(|&n| doc.get(n).get_attr("id").map_or_else(|| "?".to_owned(), str::to_owned))
            .collect();
        ids.sort();
        Some(ids)
    }

    #[test]
    fn the_elements_a_rule_selects_are_the_roots() {
        assert_eq!(roots(".d p { color: red }"), Some(vec!["p2".to_owned()]));
        assert_eq!(roots("b { color: red } #b { margin: 0 }"), Some(vec!["b".to_owned(), "b1".to_owned()]));
        assert_eq!(roots(".nope { color: red }"), Some(Vec::new()));
    }

    #[test]
    fn a_root_takes_its_subtree_with_it() {
        // `.c` selects both divs; `p` inside them is already under a root.
        assert_eq!(roots(".c { color: red } .c p { margin: 0 } b { color: blue }"), Some(vec!["a".to_owned(), "b".to_owned()]));
    }

    #[test]
    fn a_selector_list_is_the_union_of_its_selectors() {
        assert_eq!(roots("#p1, #p2 { color: red }"), Some(vec!["p1".to_owned(), "p2".to_owned()]));
    }

    #[test]
    fn a_rule_that_can_change_what_is_under_an_element_is_not_expressible() {
        for css in [
            "p::before { content: 'x' }",
            "p::first-line { color: red }",
            "::selection { color: red }",
            ":host { color: red }",
            "#p1, p::after { content: 'x' }",
        ] {
            assert_eq!(roots(css), None, "{css}");
        }
    }

    #[test]
    fn too_wide_a_change_is_left_to_the_full_cascade() {
        let css: String = (0..=MAX_RULES).map(|i| format!(".r{i} {{ color: red }} ")).collect();
        assert_eq!(roots(&css), None);
    }
}
