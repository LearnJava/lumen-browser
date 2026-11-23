//! BUG-935 срез 64, BUG-1837 — restyle roots for a change of the cascade sheet itself.
//!
//! [`lumen_css_parser::Stylesheet::changed_style_rules`] names the style rules (plain and
//! inside `@media`/`@supports` blocks) that were added, removed or moved between two versions
//! of the sheet. The elements whose computed style can differ are the ones those rules select:
//! a rule's effect on an element is decided by the element alone (and its ancestors/siblings
//! for the combinators), never by the style of another element. A rule with a pseudo-element
//! in its subject selects the element that pseudo-element originates from. Each such element
//! is returned as a root whose whole subtree is restyled — what it passes down by inheritance
//! changes too.

use std::collections::HashSet;

use lumen_css_parser::{ComplexSelector, CompoundSelector, PseudoClass, PseudoElementKind, Rule, SimpleSelector};
use lumen_dom::{Document, NodeData, NodeId};

use super::cascade_index::declaration_can_yield_quote;
use crate::rule_index::RuleIndex;
use crate::style::matches_complex;

/// More changed rules than this and the walk costs about what the full cascade does. The rules
/// are looked up through a [`RuleIndex`], so only an element's candidates are matched.
const MAX_RULES: usize = 2048;

/// Why a change of the sheet's rules is not expressed as restyle roots — what a flush logs
/// when it falls back to the full cascade.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleChangeNotExpressible {
    /// More rules changed than a walk of the document pays for.
    TooWide,
    /// A changed rule selects into a shadow tree — `:host`, `::part()`, `::slotted()` — and
    /// the roots are looked up in the light tree only.
    ShadowTree,
    /// A changed rule selects `::first-line` or `::first-letter`. Their styles are written
    /// into laid-out lines, and an element whose own style stays equal gets its previous lines
    /// carried over, the old first line among them.
    LinePseudo,
    /// A changed pseudo-element rule can open or close a quote. The quote depth of everything
    /// after its element in document order moves with it, clean subtrees a flush skips too.
    Quotes,
}

impl std::fmt::Display for RuleChangeNotExpressible {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::TooWide => "more rules changed than a root walk pays for",
            Self::ShadowTree => "a changed rule selects into a shadow tree",
            Self::LinePseudo => "a changed rule selects ::first-line or ::first-letter",
            Self::Quotes => "a changed pseudo-element rule opens or closes quotes",
        })
    }
}

/// The deep restyle roots for the style rules in `changed`, matched against `doc`'s light
/// tree.
///
/// A rule with a pseudo-element in its subject (`a::before`, `input::placeholder`) takes the
/// element the pseudo-element originates from as its root. BUG-1837: such a rule used to send
/// the flush down the full cascade, and on okko.tv that was three of the four sheet changes a
/// load could have absorbed. It need not: a root is always recomputed, a recomputed element never reports itself
/// clean (see `counters::walk`), and a box built afresh takes its pseudo-elements from the new
/// sheet. What that does not cover stays with the full cascade — [`RuleChangeNotExpressible`].
///
/// No returned root is below another one — a subtree already restyled is not walked again.
///
/// # Errors
///
/// [`RuleChangeNotExpressible`] when the roots cannot be named by looking at the elements
/// alone; the caller then recascades everything.
pub fn restyle_roots_for_rule_changes(
    doc: &Document,
    changed: &[&Rule],
) -> Result<HashSet<NodeId>, RuleChangeNotExpressible> {
    if changed.len() > MAX_RULES {
        return Err(RuleChangeNotExpressible::TooWide);
    }
    // BUG-1837: every changed rule as the elements it originates from see it — the same
    // selectors with the pseudo-elements of the subject left out, so the ordinary matcher
    // finds them. Built once per rule; the walk below then matches without allocating.
    let mut origins = Vec::with_capacity(changed.len());
    for rule in changed {
        let mut selectors = Vec::with_capacity(rule.selectors.len());
        let mut on_pseudo = false;
        for complex in &rule.selectors {
            if let Some((selector, pseudo)) = originating(complex)? {
                on_pseudo |= pseudo;
                selectors.push(selector);
            }
        }
        if on_pseudo && rule.declarations.iter().any(declaration_can_yield_quote) {
            return Err(RuleChangeNotExpressible::Quotes);
        }
        origins.push(Rule { selectors, declarations: Vec::new() });
    }
    let index = RuleIndex::build_from_indexed(origins.iter().enumerate());
    let mut roots = HashSet::new();
    let mut stack = vec![doc.root()];
    while let Some(id) = stack.pop() {
        let node = doc.get(id);
        if let NodeData::Element { name, attrs, .. } = &node.data {
            let selected = index
                .candidates(
                    name.local.as_str(),
                    node.get_attr("id"),
                    node.get_attr("class").unwrap_or("").split_whitespace(),
                    attrs,
                )
                .into_iter()
                .any(|rule| origins[rule].selectors.iter().any(|s| matches_complex(s, doc, id)));
            if selected {
                roots.insert(id);
                continue;
            }
        }
        stack.extend(node.children.iter().copied());
    }
    Ok(roots)
}

/// `complex` as the selector of the elements it originates from, with whether its subject
/// carries a pseudo-element. `None` for a selector with a pseudo-element outside the subject
/// compound: that is invalid CSS, and it matches no element and no pseudo-element.
///
/// The subject compound is taken without its `PseudoElement` parts and the rest of the
/// selector as it is — the split `style::pseudo::matches_complex_for_pseudo` makes.
fn originating(complex: &ComplexSelector) -> Result<Option<(ComplexSelector, bool)>, RuleChangeNotExpressible> {
    let compounds = std::iter::once(&complex.head).chain(complex.tail.iter().map(|(_, c)| c));
    if compounds.clone().any(|c| {
        c.parts.iter().any(|p| matches!(p, SimpleSelector::PseudoClass(PseudoClass::Host(_))))
    }) {
        return Err(RuleChangeNotExpressible::ShadowTree);
    }
    let (subject, prefix) = complex
        .tail
        .last()
        .map_or((&complex.head, &[][..]), |(_, c)| (c, &complex.tail[..complex.tail.len() - 1]));
    let mut on_pseudo = false;
    for part in &subject.parts {
        if let SimpleSelector::PseudoElement(kind) = part {
            match kind {
                PseudoElementKind::FirstLine | PseudoElementKind::FirstLetter => {
                    return Err(RuleChangeNotExpressible::LinePseudo);
                }
                PseudoElementKind::Part(_) | PseudoElementKind::Slotted(_) => {
                    return Err(RuleChangeNotExpressible::ShadowTree);
                }
                _ => on_pseudo = true,
            }
        }
    }
    let ahead = std::iter::once(&complex.head)
        .chain(prefix.iter().map(|(_, c)| c))
        .filter(|c| !std::ptr::eq(*c, subject))
        .any(|c| c.parts.iter().any(|p| matches!(p, SimpleSelector::PseudoElement(_))));
    if ahead {
        return Ok(None);
    }
    if !on_pseudo {
        return Ok(Some((complex.clone(), false)));
    }
    let stripped = CompoundSelector {
        parts: subject
            .parts
            .iter()
            .filter(|p| !matches!(p, SimpleSelector::PseudoElement(_)))
            .cloned()
            .collect(),
    };
    let mut origin = complex.clone();
    match origin.tail.last_mut() {
        Some(last) => last.1 = stripped,
        None => origin.head = stripped,
    }
    Ok(Some((origin, true)))
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
        let found = restyle_roots_for_rule_changes(&doc, &rules).ok()?;
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

    fn why(css: &str) -> Option<RuleChangeNotExpressible> {
        let doc = parse_html(HTML);
        let sheet = parse_css(css);
        let rules: Vec<&Rule> = sheet.rules.iter().collect();
        restyle_roots_for_rule_changes(&doc, &rules).err()
    }

    /// BUG-1837: a pseudo-element rule is matched against the element it originates from —
    /// with the rest of the selector and the subject's other parts as they are.
    #[test]
    fn a_pseudo_element_rule_takes_its_originating_element_as_the_root() {
        assert_eq!(roots("p::before { content: 'x' }"), Some(vec!["p1".to_owned(), "p2".to_owned()]));
        assert_eq!(roots("b::after { content: '' }"), Some(vec!["b1".to_owned()]));
        assert_eq!(roots(".c::before { content: 'x' }"), Some(vec!["a".to_owned(), "b".to_owned()]));
        assert_eq!(roots(".d p::marker { color: red }"), Some(vec!["p2".to_owned()]));
        // One selector list: the pseudo-element rule and the plain one name the same elements.
        assert_eq!(roots("#p1, p::after { content: 'x' }"), Some(vec!["p1".to_owned(), "p2".to_owned()]));
        // A bare pseudo-element originates from every element; the topmost one takes the rest.
        assert_eq!(roots("::before { content: 'x' }"), Some(vec!["?".to_owned()]));
        // The state pseudo-classes of the subject still decide: nothing is hovered here.
        assert_eq!(roots("p:hover::before { content: 'x' }"), Some(Vec::new()));
        assert_eq!(roots("input::placeholder { color: red }"), Some(Vec::new()));
    }

    /// A pseudo-element ahead of the subject is invalid CSS: the selector matches nothing.
    #[test]
    fn a_pseudo_element_outside_the_subject_selects_nothing() {
        assert_eq!(roots("a::before p { color: red } p::after { content: 'x' }"), Some(vec!["p1".to_owned(), "p2".to_owned()]));
    }

    #[test]
    fn a_rule_that_selects_into_a_shadow_tree_is_not_expressible() {
        for css in [
            ":host { color: red }",
            "#p1, :host(.x) { color: red }",
            "::part(label) { color: red }",
            "::slotted(p) { color: red }",
            ".box::part(label)::before { color: red }",
        ] {
            assert_eq!(why(css), Some(RuleChangeNotExpressible::ShadowTree), "{css}");
        }
    }

    #[test]
    fn first_line_and_first_letter_stay_with_the_full_cascade() {
        for css in ["p::first-line { color: red }", "b::first-letter { float: left }", "#p1, p::first-line { color: red }"] {
            assert_eq!(why(css), Some(RuleChangeNotExpressible::LinePseudo), "{css}");
        }
    }

    #[test]
    fn quotes_a_changed_pseudo_element_opens_stay_with_the_full_cascade() {
        for css in [
            "p::before { content: open-quote }",
            "p::after { content: no-close-quote }",
            "p::before { --mark: close-quote; content: var(--mark) }",
        ] {
            assert_eq!(why(css), Some(RuleChangeNotExpressible::Quotes), "{css}");
        }
    }

    #[test]
    fn too_wide_a_change_is_left_to_the_full_cascade() {
        let css: String = (0..=MAX_RULES).map(|i| format!(".r{i} {{ color: red }} ")).collect();
        assert_eq!(roots(&css), None);
    }
}
