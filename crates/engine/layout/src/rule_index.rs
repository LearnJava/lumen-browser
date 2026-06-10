//! Selector rule index: buckets stylesheet rules by the rightmost (subject)
//! simple selector so `compute_style` tests only candidate rules per node,
//! not all rules. Pure performance optimisation — correctness is unchanged
//! because the candidate set is always a superset of the matching rules.
//!
//! See `docs/tasks/p1-selector-rule-index.md` for the performance rationale.

use std::collections::HashMap;
use lumen_css_parser::{CompoundSelector, PseudoClass, SimpleSelector, Stylesheet};

/// Opaque index into `Stylesheet.rules`.
type RuleIdx = usize;

/// Subject-keyed rule index for the top-level `rules` vec of a stylesheet.
///
/// Rules in `sheet.layers`, `sheet.media_rules`, `sheet.scope_rules`,
/// `sheet.supports_rules`, and `sheet.container_rules` are NOT indexed
/// (Phase 1 scope) — they are still tested brute-force by the existing loops
/// in `compute_style` / `apply_container_rules`, which is fine because they
/// are typically a small fraction of total rules.
pub struct RuleIndex {
    by_id: HashMap<String, Vec<RuleIdx>>,
    by_class: HashMap<String, Vec<RuleIdx>>,
    by_type: HashMap<String, Vec<RuleIdx>>,
    /// Rules whose subject compound has no id/class/type discriminator — must
    /// be tested against every node (universal, attribute-only, functional
    /// pseudo-class in subject position, etc.).
    universal: Vec<RuleIdx>,
}

// ── helpers ──────────────────────────────────────────────────────────────────

/// Returns the rightmost (subject) compound of a complex selector.
///
/// Our CSS parser stores head = leftmost compound; tail goes rightward.
/// So the subject is `tail.last()`, or `head` if the tail is empty.
fn subject(c: &lumen_css_parser::ComplexSelector) -> &CompoundSelector {
    c.tail.last().map(|(_, comp)| comp).unwrap_or(&c.head)
}

/// True when the pseudo-class requires evaluating inner selector lists, making
/// it impossible to bucket by structural node properties alone.
fn pc_is_functional(pc: &PseudoClass) -> bool {
    matches!(
        pc,
        PseudoClass::Not(_)
            | PseudoClass::Is(_)
            | PseudoClass::Where(_)
            | PseudoClass::Has(_)
            | PseudoClass::NthChild(_, Some(_))
            | PseudoClass::NthLastChild(_, Some(_))
    )
}

/// Returns the strongest indexable key for a subject compound.
///
/// Priority: Id > first Class > Type > Universal.
/// If the compound contains any functional pseudo-class (one whose matching
/// depends on inner selector lists), we conservatively return Universal to
/// avoid missing a match — `matches_complex` will still validate everything.
fn subject_key(comp: &CompoundSelector) -> SubjectKey<'_> {
    // Functional pseudo in subject → cannot index by structural key alone.
    if comp.parts.iter().any(|p| {
        matches!(p, SimpleSelector::PseudoClass(pc) if pc_is_functional(pc))
    }) {
        return SubjectKey::Universal;
    }
    for p in &comp.parts {
        if let SimpleSelector::Id(s) = p {
            return SubjectKey::Id(s);
        }
    }
    for p in &comp.parts {
        if let SimpleSelector::Class(s) = p {
            return SubjectKey::Class(s);
        }
    }
    for p in &comp.parts {
        if let SimpleSelector::Type(s) = p {
            return SubjectKey::Type(s);
        }
    }
    SubjectKey::Universal
}

enum SubjectKey<'a> {
    Id(&'a str),
    Class(&'a str),
    Type(&'a str),
    Universal,
}

// ── RuleIndex ─────────────────────────────────────────────────────────────────

impl RuleIndex {
    /// Empty index — used as the initial value of the thread-local cache.
    pub fn empty() -> Self {
        Self {
            by_id: HashMap::new(),
            by_class: HashMap::new(),
            by_type: HashMap::new(),
            universal: Vec::new(),
        }
    }

    /// Builds an index over the top-level rules of `sheet`.
    ///
    /// O(rules × selectors_per_rule). Called at most once per stylesheet per
    /// layout pass thanks to the thread-local cache in `compute_style`.
    pub fn build(sheet: &Stylesheet) -> Self {
        let mut idx = Self::empty();
        for (rule_idx, rule) in sheet.rules.iter().enumerate() {
            for sel in &rule.selectors {
                match subject_key(subject(sel)) {
                    SubjectKey::Id(id) => {
                        idx.by_id.entry(id.to_owned()).or_default().push(rule_idx);
                    }
                    SubjectKey::Class(cls) => {
                        idx.by_class.entry(cls.to_owned()).or_default().push(rule_idx);
                    }
                    SubjectKey::Type(tag) => {
                        idx.by_type.entry(tag.to_owned()).or_default().push(rule_idx);
                    }
                    SubjectKey::Universal => {
                        idx.universal.push(rule_idx);
                    }
                }
            }
        }
        // Deduplicate each bucket (a rule with multiple selectors in the same
        // bucket would otherwise be tested twice for the same node).
        for v in idx.by_id.values_mut() {
            v.sort_unstable();
            v.dedup();
        }
        for v in idx.by_class.values_mut() {
            v.sort_unstable();
            v.dedup();
        }
        for v in idx.by_type.values_mut() {
            v.sort_unstable();
            v.dedup();
        }
        idx.universal.sort_unstable();
        idx.universal.dedup();
        idx
    }

    /// Returns the deduplicated, sorted candidate rule indices for a node.
    ///
    /// A candidate is any rule whose subject-key is compatible with the node's
    /// `tag`, `id`, and `class` list. The full `matches_complex` check is
    /// still required for each candidate — this is only a pre-filter.
    pub fn candidates(
        &self,
        tag: &str,
        id: Option<&str>,
        classes: &[&str],
    ) -> Vec<RuleIdx> {
        // Merge from all relevant buckets into a BTreeSet for dedup+sort.
        let mut set = std::collections::BTreeSet::new();
        // type bucket
        if let Some(v) = self.by_type.get(tag) {
            set.extend(v.iter().copied());
        }
        // id bucket
        if let Some(id_str) = id
            && let Some(v) = self.by_id.get(id_str)
        {
            set.extend(v.iter().copied());
        }
        // class buckets (one per class token)
        for &cls in classes {
            if let Some(v) = self.by_class.get(cls) {
                set.extend(v.iter().copied());
            }
        }
        // universal (always-check)
        set.extend(self.universal.iter().copied());
        set.into_iter().collect()
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_css_parser::parse;

    fn build(css: &str) -> RuleIndex {
        RuleIndex::build(&parse(css))
    }

    #[test]
    fn by_id_bucket() {
        let idx = build("#foo { color: red }");
        assert!(idx.by_id.contains_key("foo"));
        assert!(idx.by_class.is_empty());
        assert!(idx.by_type.is_empty());
        assert!(idx.universal.is_empty());
    }

    #[test]
    fn by_class_bucket() {
        let idx = build(".card { color: red }");
        assert!(idx.by_class.contains_key("card"));
        assert!(idx.by_id.is_empty());
        assert!(idx.by_type.is_empty());
        assert!(idx.universal.is_empty());
    }

    #[test]
    fn by_type_bucket() {
        let idx = build("div { color: red }");
        assert!(idx.by_type.contains_key("div"));
        assert!(idx.by_id.is_empty());
        assert!(idx.by_class.is_empty());
        assert!(idx.universal.is_empty());
    }

    /// `.a.b` subject is `.a` (first class) — indexed under `.a`.
    /// A node with only `.a` (no `.b`) is a candidate but `matches_complex`
    /// will reject it.
    #[test]
    fn multi_class_indexes_under_first_class() {
        let sheet = parse(".a.b { color: red }");
        let idx = RuleIndex::build(&sheet);
        assert!(idx.by_class.contains_key("a"), "must index under first class");
        assert!(!idx.by_class.contains_key("b"), "second class must not create a separate bucket entry");

        // Node with class="a" only → is a candidate (will be rejected later by matches_complex)
        let cands = idx.candidates("span", None, &["a"]);
        assert!(!cands.is_empty(), "should be a candidate when only .a matches");
    }

    /// `.card .title` → subject = `.title`, indexed in by_class["title"].
    #[test]
    fn descendant_selector_uses_subject_compound() {
        let idx = build(".card .title { color: red }");
        assert!(idx.by_class.contains_key("title"), "must bucket under subject .title");
        assert!(!idx.by_class.contains_key("card"), "ancestor class must not create bucket");

        let cands = idx.candidates("span", None, &["title"]);
        assert!(!cands.is_empty());
        let cands_no_title = idx.candidates("span", None, &["card"]);
        assert!(cands_no_title.is_empty(), "node without .title is not a candidate");
    }

    /// `:is(.a, .b) span` → subject = `span` (type), not universal.
    #[test]
    fn functional_pseudo_in_ancestor_not_in_subject() {
        let idx = build(":is(.a, .b) span { color: red }");
        // subject compound is just `span` — no functional pseudo there
        assert!(idx.by_type.contains_key("span"), "must bucket under type span");
        assert!(idx.universal.is_empty(), "should NOT be universal");
    }

    /// `div:is(.x)` — functional pseudo IS in the subject → universal bucket.
    #[test]
    fn functional_pseudo_in_subject_goes_to_universal() {
        let idx = build("div:is(.x) { color: red }");
        // div has a functional pseudo in subject → conservative universal
        assert!(!idx.universal.is_empty(), "must be universal due to functional pseudo in subject");
    }

    /// `*` and `[hidden]` → universal bucket.
    #[test]
    fn universal_and_attribute_go_to_universal() {
        let idx_star = build("* { margin: 0 }");
        assert!(!idx_star.universal.is_empty(), "* must be universal");

        let idx_attr = build("[hidden] { display: none }");
        assert!(!idx_attr.universal.is_empty(), "[attr] must be universal");
    }

    /// `candidates()` merges type + class + id + universal and deduplicates.
    #[test]
    fn candidates_dedup_and_sort() {
        // Same rule_idx appears from type AND universal (simulated by two selectors).
        let sheet = parse("div, * { color: red }");
        let idx = RuleIndex::build(&sheet);
        // rule 0 should appear from by_type["div"] AND universal; after dedup only once.
        let cands = idx.candidates("div", None, &[]);
        let rule0_count = cands.iter().filter(|&&r| r == 0).count();
        assert_eq!(rule0_count, 1, "dedup must not repeat rule_idx");
    }
}
