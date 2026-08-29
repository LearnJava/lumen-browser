//! BUG-341 — фан-аут рестайла: какие узлы придётся пересчитать после смены
//! интерактивного состояния `:hover`/`:focus`/`:active` (срезы S7/S14) и после
//! мутации DOM — атрибут, класс, структура (срез S17). Индексы
//! [`StateRestyleIndex`]/[`NodeRestyleIndex`] строятся один раз на проход и
//! переиспользуются всеми осями.
//!
//! Перенесено батчем SPLIT-ST10 из `crates/engine/layout/src/style.rs`
//! (анкер `fn ancestor_chain_inclusive`) без правок тел: изменена только
//! видимость `stylesheet_needs_state_fanout`, у которой вызыватели только в
//! тестах.

use std::collections::HashSet;

use lumen_css_parser::{
    Combinator, ComplexSelector, CompoundSelector, PseudoClass, SimpleSelector, Stylesheet,
};
use lumen_dom::{Document, NodeData, NodeId};

use crate::style::matches_simple;

/// `node`'s ancestor chain, root-first, `node` itself last. Empty if `node` is
/// `None`.
fn ancestor_chain_inclusive(doc: &Document, node: Option<NodeId>) -> Vec<NodeId> {
    let Some(node) = node else { return Vec::new() };
    let mut chain = Vec::new();
    let mut cur = Some(node);
    while let Some(n) = cur {
        chain.push(n);
        cur = doc.get(n).parent;
    }
    chain.reverse();
    chain
}

/// BUG-341 S7 — true if a compound selector's matching result can depend on
/// the dynamic interactive-state pseudo-classes (`:hover`/`:focus`/`:active`/
/// `:focus-within`/`:focus-visible`), directly or through a nested selector
/// list (`:not()`/`:is()`/`:where()`/`:host()`/the `of <selector-list>` clause
/// of `:nth-child()`/`:nth-last-child()`). Conservative in the nested-list
/// direction: any dynamic-state pseudo appearing anywhere in the inner list
/// counts, regardless of the inner list's own combinators — the question this
/// answers is only "can this compound's boolean flip", not "for which nodes".
fn compound_depends_on_dynamic_state(compound: &CompoundSelector) -> bool {
    compound.parts.iter().any(simple_selector_depends_on_dynamic_state)
}

fn simple_selector_depends_on_dynamic_state(part: &SimpleSelector) -> bool {
    match part {
        SimpleSelector::PseudoClass(pc) => pseudo_class_depends_on_dynamic_state(pc),
        _ => false,
    }
}

fn pseudo_class_depends_on_dynamic_state(pc: &PseudoClass) -> bool {
    match pc {
        PseudoClass::Hover
        | PseudoClass::Focus
        | PseudoClass::Active
        | PseudoClass::FocusWithin
        | PseudoClass::FocusVisible => true,
        PseudoClass::Not(list) | PseudoClass::Is(list) | PseudoClass::Where(list) => {
            list.iter().any(complex_selector_depends_on_dynamic_state)
        }
        PseudoClass::Host(Some(list)) => list.iter().any(complex_selector_depends_on_dynamic_state),
        PseudoClass::NthChild(_, Some(list)) | PseudoClass::NthLastChild(_, Some(list)) => {
            list.iter().any(complex_selector_depends_on_dynamic_state)
        }
        _ => false,
    }
}

fn complex_selector_depends_on_dynamic_state(c: &ComplexSelector) -> bool {
    compound_depends_on_dynamic_state(&c.head)
        || c.tail.iter().any(|(_, comp)| compound_depends_on_dynamic_state(comp))
}

/// BUG-341 S7 — true if `complex` contains a `:has()` anywhere whose relative
/// selector list depends on dynamic interactive state (`:has(:hover)` and
/// friends). `:has()` searches *outward* from its subject (descendants and/or
/// following siblings, depending on the relative selector's own leading
/// combinator) — a direction this v1 narrowing pass doesn't attempt to model.
/// Selectors matching this predicate always force the conservative
/// widen-to-parent fanout in [`selector_needs_state_fanout`], same as before
/// S7 (no behaviour change for stylesheets using this pattern).
fn complex_selector_has_dynamic_has(c: &ComplexSelector) -> bool {
    fn compound_has_dynamic_has(compound: &CompoundSelector) -> bool {
        compound.parts.iter().any(|p| match p {
            SimpleSelector::PseudoClass(PseudoClass::Has(list)) => {
                list.iter().any(|rs| complex_selector_depends_on_dynamic_state(&rs.selector))
            }
            SimpleSelector::PseudoClass(
                PseudoClass::Not(list) | PseudoClass::Is(list) | PseudoClass::Where(list),
            ) => list.iter().any(complex_selector_has_dynamic_has),
            _ => false,
        })
    }
    compound_has_dynamic_has(&c.head) || c.tail.iter().any(|(_, comp)| compound_has_dynamic_has(comp))
}

/// BUG-341 S7 — true if `complex` needs the v1 widen-to-parent behaviour: a
/// compound depending on dynamic interactive state is followed (anywhere on
/// the path to the subject) by a sibling combinator (`+`/`~`), so a state flip
/// on that compound's matched node could restyle a sibling or a descendant of
/// a sibling — outside the flipped node's own subtree. A dynamic-state
/// compound followed only by descendant/child combinators (or in subject
/// position, with nothing after it) stays within the flipped node's own
/// subtree and needs no widening. `:has()` selectors depending on dynamic
/// state always widen (see [`complex_selector_has_dynamic_has`]).
fn selector_needs_state_fanout(complex: &ComplexSelector) -> bool {
    if complex_selector_has_dynamic_has(complex) {
        return true;
    }
    let mut compounds: Vec<&CompoundSelector> = Vec::with_capacity(1 + complex.tail.len());
    let mut combinators: Vec<Combinator> = Vec::with_capacity(complex.tail.len());
    compounds.push(&complex.head);
    for (comb, comp) in &complex.tail {
        combinators.push(*comb);
        compounds.push(comp);
    }
    for (i, compound) in compounds.iter().enumerate() {
        if !compound_depends_on_dynamic_state(compound) {
            continue;
        }
        if combinators[i..].iter().any(|c| matches!(c, Combinator::NextSibling | Combinator::LaterSibling)) {
            return true;
        }
    }
    false
}

fn rules_need_state_fanout(rules: &[lumen_css_parser::Rule]) -> bool {
    rules.iter().any(|r| r.selectors.iter().any(selector_needs_state_fanout))
}

/// BUG-341 S14 — collect every compound of `complex` whose matching result can
/// flip with the interactive state of the *node bound to that compound*.
///
/// All the nested-list forms [`pseudo_class_depends_on_dynamic_state`] looks
/// through (`:not()`/`:is()`/`:where()`/`:host()`/`:nth-child(… of …)`) evaluate
/// their inner selector against the same subject node as the compound that
/// carries them, so "this compound depends on dynamic state" and "the node this
/// compound is matched against is the node whose state flips" are the same
/// statement. `:has()` is the one exception (it binds state to a *different*
/// node) and is excluded from the narrowing entirely — see
/// [`StateRestyleIndex::conservative`].
fn collect_state_compounds<'a>(complex: &'a ComplexSelector, out: &mut Vec<&'a CompoundSelector>) {
    if compound_depends_on_dynamic_state(&complex.head) {
        out.push(&complex.head);
    }
    for (_, compound) in &complex.tail {
        if compound_depends_on_dynamic_state(compound) {
            out.push(compound);
        }
    }
}

fn collect_rules_state_compounds<'a>(
    rules: &'a [lumen_css_parser::Rule],
    out: &mut Vec<&'a CompoundSelector>,
) {
    for rule in rules {
        for selector in &rule.selectors {
            collect_state_compounds(selector, out);
        }
    }
}

/// BUG-341 S14 — could `compound` match `node` *after* an interactive-state
/// flip on `node`, ignoring what that state currently is?
///
/// Every part must match structurally, except the two kinds whose value this
/// question deliberately leaves open: the dynamic-state pseudo-classes
/// themselves (their boolean is exactly what is being flipped) and pseudo-
/// elements (stripped by the real matcher — [`matches_complex_for_pseudo`] —
/// before the compound is matched against an element, so treating them as
/// matching keeps `.tab:hover::before` attributable to `.tab`).
///
/// Over-approximates in one direction only: a compound whose dynamic state
/// hides inside a nested list (`:is(.tab:hover, .x)`) has *all* its state-
/// carrying parts treated as "possible", so it matches any element. That costs
/// narrowing, never correctness.
fn compound_could_match_after_state_flip(
    compound: &CompoundSelector,
    doc: &Document,
    node: NodeId,
) -> bool {
    let NodeData::Element { name, attrs } = &doc.get(node).data else {
        return false;
    };
    compound.parts.iter().all(|part| match part {
        SimpleSelector::PseudoElement(_) => true,
        SimpleSelector::PseudoClass(pc) if pseudo_class_depends_on_dynamic_state(pc) => true,
        other => matches_simple(other, doc, node, &name.local, attrs),
    })
}

/// BUG-341 S7/S14 — everything [`restyle_root_set_for_state_change`] needs to
/// know about the stylesheet in play, computed once per layout pass and reused
/// across the three interactive-state axes (hover/focus/active) since the
/// stylesheet's shape does not change between them.
pub struct StateRestyleIndex<'a> {
    /// S7 — widen each flipped node's invalidation to its parent's subtree
    /// (some selector reaches a sibling from a dynamic-state compound).
    needs_fanout: bool,
    /// S14 — the per-node narrowing below is unsound for this document/sheet
    /// pair, so every flipped node stays in the root set (pre-S14 behaviour).
    /// Set by `:has()` depending on dynamic state (state on one node restyles
    /// an arbitrarily distant ancestor) or by the presence of any shadow root
    /// (shadow-tree sheets are not scanned here — same carve-out S7 made).
    conservative: bool,
    /// S14 — every compound in `sheet` whose match can flip with the state of
    /// the node it is matched against ([`collect_state_compounds`]).
    state_compounds: Vec<&'a CompoundSelector>,
}

impl StateRestyleIndex<'_> {
    /// S7 — whether a flipped node's invalidation widens to its parent.
    pub fn needs_fanout(&self) -> bool {
        self.needs_fanout
    }

    /// S14 — whether per-node narrowing is disabled for this document/sheet.
    pub fn is_conservative(&self) -> bool {
        self.conservative
    }

    /// S14 — number of state-dependent compounds the narrowing tests each
    /// flipped node against. Exposed for the count-based regression gates.
    pub fn state_compound_count(&self) -> usize {
        self.state_compounds.len()
    }

    /// S14 — can an interactive-state flip on `node` change *any* computed
    /// style in the document?
    ///
    /// It can only do so through a compound that (a) depends on dynamic state
    /// and (b) is matched against `node` itself; everything such a compound can
    /// reach — its own subject, and descendants via `N:hover X` — is inside
    /// `node`'s own subtree (sibling reach is what `needs_fanout` widens for,
    /// and `:has()` reach is what `conservative` disables narrowing for). So if
    /// no state-dependent compound can even structurally match `node`, the flip
    /// is unobservable and `node` contributes nothing to the restyle root set.
    pub fn state_flip_can_matter(&self, doc: &Document, node: NodeId) -> bool {
        if self.conservative {
            return true;
        }
        self.state_compounds
            .iter()
            .any(|c| compound_could_match_after_state_flip(c, doc, node))
    }
}

/// BUG-341 S7 — true if any selector anywhere in `sheet` (top-level rules plus
/// every `@layer`/`@media`/`@supports`/`@scope`/`@starting-style`/`@container`
/// block) needs [`selector_needs_state_fanout`]'s widen-to-parent behaviour.
/// Scans unconditionally on `@media`/`@supports`/`@container` activation (a
/// selector inside a currently-inactive block still counts) — safe (never
/// narrows incorrectly) and avoids threading viewport/dark-mode state into
/// this check.
/// Test-only: the sheet half of [`restyle_state_index`]'s `needs_fanout`
/// computation, kept as a named predicate because the S7 unit tests below
/// assert on it selector-shape by selector-shape. Production code takes the
/// single fused scan in `restyle_state_index` instead.
#[cfg(test)]
pub(in crate::style) fn stylesheet_needs_state_fanout(sheet: &Stylesheet) -> bool {
    stylesheet_rule_groups(sheet).any(rules_need_state_fanout)
}

/// Every rule list in `sheet`, top-level plus every conditional/grouping
/// at-rule block. Iterated unconditionally — see
/// [`stylesheet_needs_state_fanout`] for why an inactive `@media` block still
/// counts.
fn stylesheet_rule_groups(sheet: &Stylesheet) -> impl Iterator<Item = &[lumen_css_parser::Rule]> {
    std::iter::once(sheet.rules.as_slice())
        .chain(sheet.media_rules.iter().map(|m| m.rules.as_slice()))
        .chain(sheet.layers.iter().map(|l| l.rules.as_slice()))
        .chain(sheet.supports_rules.iter().map(|s| s.rules.as_slice()))
        .chain(sheet.scope_rules.iter().map(|s| s.rules.as_slice()))
        .chain(sheet.starting_style_rules.iter().map(|s| s.rules.as_slice()))
        .chain(sheet.container_rules.iter().map(|c| c.rules.as_slice()))
}

fn rules_have_dynamic_has(rules: &[lumen_css_parser::Rule]) -> bool {
    rules.iter().any(|r| r.selectors.iter().any(complex_selector_has_dynamic_has))
}

/// True if `doc` has any shadow host. Shadow-tree stylesheets
/// ([`SHADOW_SHEETS`]) are not scanned by [`stylesheet_needs_state_fanout`] —
/// modelling their per-host scoping would need the narrowing check to run
/// per-node instead of once per pass, deferred until a fixture demonstrates
/// the benefit is worth that complexity. A document with any shadow root
/// therefore always takes the conservative widen-to-parent path, matching
/// this engine's pre-S7 behaviour exactly (no regression, just no narrowing
/// win for shadow-DOM-heavy pages yet).
fn document_has_shadow_roots(doc: &Document) -> bool {
    (0..doc.len()).any(|i| doc.is_shadow_host(NodeId::from_index(i)))
}

/// BUG-341 S7/S14 — builds the [`StateRestyleIndex`] for one layout pass.
///
/// Computed once per interactive-state transition and reused across the
/// hover/focus/active axes — each calls [`restyle_root_set_for_state_change`]
/// separately, but the stylesheet/shadow-DOM shape doesn't change between them.
/// Costs one scan of the sheet's selectors (the same scan S7 already did for
/// `needs_fanout` alone), then one structural match per flipped node per
/// state-dependent compound.
pub fn restyle_state_index<'a>(doc: &Document, sheet: &'a Stylesheet) -> StateRestyleIndex<'a> {
    let shadow = document_has_shadow_roots(doc);
    let mut needs_fanout = false;
    let mut conservative = shadow;
    let mut state_compounds = Vec::new();
    for rules in stylesheet_rule_groups(sheet) {
        needs_fanout |= rules_need_state_fanout(rules);
        conservative |= rules_have_dynamic_has(rules);
        collect_rules_state_compounds(rules, &mut state_compounds);
    }
    StateRestyleIndex { needs_fanout: needs_fanout || shadow, conservative, state_compounds }
}

/// BUG-341 S3/S7 — restyle root-set (brief §4) for an interactive-state
/// transition (`:hover` / `:focus` / `:active`, each read via
/// [`set_interactive_state`]).
///
/// `:hover`/`:active` match the affected element *and all its ancestors*
/// (CSS Selectors L4 §4.3/§4.5); `:focus-within` matches an ancestor of the
/// focused element the same way. Moving the state from `prev` to `new`
/// therefore flips the pseudo-class boolean on every node strictly below
/// their lowest common ancestor (the LCA's own boolean is unaffected — it was
/// already true, and stays true, for either "some descendant is `prev`" or
/// "some descendant is `new`").
///
/// `index` — from [`restyle_state_index`] — controls two independent
/// narrowings applied to every flipped node `N`:
///
/// * [`StateRestyleIndex::needs_fanout`] (S7) selects how wide `N`'s
///   invalidation is: `true` invalidates `N`'s *parent's* whole subtree (S3's
///   conservative over-approximation, covering `N:hover + X`, `N:hover ~ X`);
///   `false` invalidates only `N` itself — sound exactly when no selector
///   anywhere needs the wider fanout, since a descendant combinator after a
///   dynamic-state compound (`N:hover X`) already resolves within `N`'s own
///   subtree without any widening.
/// * [`StateRestyleIndex::state_flip_can_matter`] (S14) drops `N` from the set
///   entirely when no state-dependent compound in the sheet can even match it.
///   This is what keeps a "nothing was hovered → deep element is hovered"
///   transition from invalidating the whole document: `:hover` does flip on
///   every ancestor up to the root, but on a sheet whose only hover rules are
///   `button:hover` / `.tab-row:hover` none of those ancestors can observe it.
///
/// Returns an empty set for a no-op transition (`prev == new`), and — since
/// S14 — also for a transition no selector in the sheet can react to.
pub fn restyle_root_set_for_state_change(
    doc: &Document,
    prev: Option<NodeId>,
    new: Option<NodeId>,
    index: &StateRestyleIndex<'_>,
) -> HashSet<NodeId> {
    let mut set = HashSet::new();
    if prev == new {
        return set;
    }
    let prev_chain = ancestor_chain_inclusive(doc, prev);
    let new_chain = ancestor_chain_inclusive(doc, new);
    let common = prev_chain
        .iter()
        .zip(new_chain.iter())
        .take_while(|(a, b)| a == b)
        .count();
    let root_for = |n: NodeId| if index.needs_fanout { doc.get(n).parent.unwrap_or(n) } else { n };
    for &n in prev_chain[common..].iter().chain(new_chain[common..].iter()) {
        if index.state_flip_can_matter(doc, n) {
            set.insert(root_for(n));
        }
    }
    set
}

/// BUG-341 S17 — true if `complex` contains a `:has()` anywhere, regardless of
/// what it looks for.
///
/// `:has()` binds one node's match result to *other* nodes' state, in the one
/// direction [`restyle_root_set_for_node_change`]'s subtree-shaped root-set
/// cannot express (BUG-348/BUG-349) — the affected ancestor can sit
/// arbitrarily far above the mutated node, not just at its immediate parent.
/// [`restyle_node_index`] uses this to set
/// [`NodeRestyleIndex::has_has_dependency`], which makes
/// [`restyle_root_set_for_node_change`] widen to the whole document instead of
/// the parent while any selector in the sheet uses `:has()` (BUG-349's fix).
fn complex_selector_has_any_has(c: &ComplexSelector) -> bool {
    fn compound_has_any_has(compound: &CompoundSelector) -> bool {
        compound.parts.iter().any(|p| match p {
            SimpleSelector::PseudoClass(PseudoClass::Has(_)) => true,
            SimpleSelector::PseudoClass(
                PseudoClass::Not(list) | PseudoClass::Is(list) | PseudoClass::Where(list),
            ) => list.iter().any(complex_selector_has_any_has),
            SimpleSelector::PseudoClass(
                PseudoClass::NthChild(_, Some(list)) | PseudoClass::NthLastChild(_, Some(list)),
            ) => list.iter().any(complex_selector_has_any_has),
            _ => false,
        })
    }
    compound_has_any_has(&c.head) || c.tail.iter().any(|(_, comp)| compound_has_any_has(comp))
}

/// BUG-341 S17 — true if `complex` uses `:nth-child(… of S)` /
/// `:nth-last-child(… of S)`.
///
/// That form makes one element's match depend on which of its *siblings* match
/// `S`, i.e. on a sibling's attributes, with no sibling combinator anywhere in
/// the selector to signal it. It is the one shape
/// [`collect_sibling_source_compounds`] cannot see, so its presence disables the
/// narrowing wholesale.
fn complex_selector_has_nth_of(c: &ComplexSelector) -> bool {
    fn compound_has_nth_of(compound: &CompoundSelector) -> bool {
        compound.parts.iter().any(|p| match p {
            SimpleSelector::PseudoClass(
                PseudoClass::NthChild(_, Some(_)) | PseudoClass::NthLastChild(_, Some(_)),
            ) => true,
            SimpleSelector::PseudoClass(
                PseudoClass::Not(list) | PseudoClass::Is(list) | PseudoClass::Where(list),
            ) => list.iter().any(complex_selector_has_nth_of),
            _ => false,
        })
    }
    compound_has_nth_of(&c.head) || c.tail.iter().any(|(_, comp)| compound_has_nth_of(comp))
}

/// BUG-341 S17 — collect every compound of `complex` that is followed, anywhere
/// on the path to the subject, by a sibling combinator (`+`/`~`).
///
/// These are exactly the compounds through which a change on the node they
/// match can reach *outside* that node's own subtree: `X + Y`, `X ~ Y`, and
/// `X + Y Z` all restyle nodes that are not descendants of `X`'s match. A
/// compound followed only by descendant/child combinators (or in subject
/// position) resolves entirely within its own match's subtree, which the
/// root-set already covers by putting the node itself in.
fn collect_sibling_source_compounds<'a>(
    complex: &'a ComplexSelector,
    out: &mut Vec<&'a CompoundSelector>,
) {
    let mut compounds: Vec<&CompoundSelector> = Vec::with_capacity(1 + complex.tail.len());
    compounds.push(&complex.head);
    let mut combinators: Vec<Combinator> = Vec::with_capacity(complex.tail.len());
    for (comb, comp) in &complex.tail {
        combinators.push(*comb);
        compounds.push(comp);
    }
    for (i, compound) in compounds.iter().enumerate() {
        if combinators[i..]
            .iter()
            .any(|c| matches!(c, Combinator::NextSibling | Combinator::LaterSibling))
        {
            out.push(compound);
        }
    }
}

/// True when `part` is keyed on the attribute named `attr` — i.e. writing that
/// attribute is what could flip this simple selector's result.
fn simple_selector_keys_on_attr(part: &SimpleSelector, attr: &str) -> bool {
    match part {
        SimpleSelector::Class(_) => attr.eq_ignore_ascii_case("class"),
        SimpleSelector::Id(_) => attr.eq_ignore_ascii_case("id"),
        SimpleSelector::Attribute(a) => a.name.eq_ignore_ascii_case(attr),
        _ => false,
    }
}

/// BUG-341 S17 — could `compound` match `node` *after* a write to `node`'s
/// `attr` attribute, ignoring what that attribute now holds?
///
/// The S14 shape ([`compound_could_match_after_state_flip`]) with the dynamic-
/// state pseudo-classes swapped for "keyed on `attr`": every part must match
/// structurally, except the parts whose value the write is what's in question
/// (which are treated as possible), pseudo-elements (stripped by the real
/// matcher before an element is tested) and pseudo-classes in general — a
/// pseudo-class may read the mutated attribute itself (`:checked` reads
/// `checked`, `:placeholder-shown` reads `value`) or hide an attribute-keyed
/// selector inside a nested list, so all of them are treated as possible.
///
/// Over-approximates in one direction only — a compound reported as "could
/// match" costs narrowing, never correctness.
fn compound_could_match_after_attr_change(
    compound: &CompoundSelector,
    doc: &Document,
    node: NodeId,
    attr: &str,
) -> bool {
    let NodeData::Element { name, attrs } = &doc.get(node).data else {
        return false;
    };
    compound.parts.iter().all(|part| match part {
        SimpleSelector::PseudoElement(_) | SimpleSelector::PseudoClass(_) => true,
        p if simple_selector_keys_on_attr(p, attr) => true,
        other => matches_simple(other, doc, node, &name.local, attrs),
    })
}

/// BUG-341 S17 — what [`restyle_root_set_for_node_change`] needs to know about
/// the stylesheet in play, computed once per layout pass.
///
/// The DOM-mutation counterpart of [`StateRestyleIndex`], and built from the
/// same single scan over every rule list in the sheet.
pub struct NodeRestyleIndex<'a> {
    /// Every compound in `sheet` from which a sibling combinator is reachable
    /// ([`collect_sibling_source_compounds`]).
    sibling_sources: Vec<&'a CompoundSelector>,
    /// The per-node narrowing below is unsound for this document/sheet pair, so
    /// every changed node widens to its parent (pre-S17 behaviour). Set by
    /// `:nth-child(… of S)` (sibling reach with no combinator to see it) or by
    /// the presence of any shadow root (shadow-tree sheets are not scanned here
    /// — the same carve-out S7 made). `:has()` is handled separately by
    /// [`has_dependent`](Self::has_dependent) — widening to the parent is not
    /// enough for it (BUG-349).
    conservative: bool,
    /// BUG-349 — `sheet` contains a `:has()` selector anywhere. A `:has()`
    /// match can flip on an ancestor arbitrarily far above the mutated node —
    /// not just its parent — so [`restyle_root_set_for_node_change`] widens
    /// every reported change to the whole document while this is set, instead
    /// of the parent-only widening `conservative` triggers on its own. No
    /// `:has()`-dependency index exists yet to narrow this further (see
    /// BUG-349's suggested fix direction for that follow-up).
    has_dependent: bool,
}

impl NodeRestyleIndex<'_> {
    /// Whether per-node narrowing is disabled for this document/sheet pair.
    pub fn is_conservative(&self) -> bool {
        self.conservative
    }

    /// BUG-349 — whether `sheet` contains a `:has()` selector, forcing
    /// [`restyle_root_set_for_node_change`] to widen every change to the whole
    /// document.
    pub fn has_has_dependency(&self) -> bool {
        self.has_dependent
    }

    /// Number of sibling-reachable compounds the narrowing tests each changed
    /// node against. Exposed for the count-based regression gates.
    pub fn sibling_source_count(&self) -> usize {
        self.sibling_sources.len()
    }

    /// Can a write to `node`'s `attr` attribute change the computed style of
    /// anything *outside* `node`'s own subtree?
    ///
    /// Only through a compound that (a) is followed by a sibling combinator and
    /// (b) can match `node` itself. If no such compound exists, every rule the
    /// write can affect resolves inside `node`'s subtree, and the root-set needs
    /// `node` alone rather than its parent.
    pub fn attr_change_needs_fanout(&self, doc: &Document, node: NodeId, attr: &str) -> bool {
        if self.conservative {
            return true;
        }
        self.sibling_sources
            .iter()
            .any(|c| compound_could_match_after_attr_change(c, doc, node, attr))
    }
}

/// BUG-341 S17 — builds the [`NodeRestyleIndex`] for one layout pass.
///
/// Costs one scan of the sheet's selectors (the same shape as
/// [`restyle_state_index`]), then one structural match per changed node per
/// sibling-reachable compound.
pub fn restyle_node_index<'a>(doc: &Document, sheet: &'a Stylesheet) -> NodeRestyleIndex<'a> {
    let mut conservative = document_has_shadow_roots(doc);
    let mut has_dependent = false;
    let mut sibling_sources = Vec::new();
    for rules in stylesheet_rule_groups(sheet) {
        for rule in rules {
            for selector in &rule.selectors {
                let has_has = complex_selector_has_any_has(selector);
                has_dependent |= has_has;
                conservative |= has_has;
                conservative |= complex_selector_has_nth_of(selector);
                collect_sibling_source_compounds(selector, &mut sibling_sources);
            }
        }
    }
    NodeRestyleIndex { sibling_sources, conservative, has_dependent }
}

/// BUG-341 S17 — one reported DOM mutation, as
/// [`restyle_root_set_for_node_change`] needs to see it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeChange<'a> {
    /// The attribute named `.0` was written to, or removed from, the node. The
    /// name is what lets the root-set ask which selectors could possibly react.
    Attr(&'a str),
    /// Something else changed, or the source cannot name what changed: a child
    /// list moved (`:nth-child`, `:empty` and sibling combinators all react to
    /// that, and no attribute name describes it), or the mutation came from a
    /// tracker that does not record names (page-side JS). Always takes the
    /// conservative widen-to-parent path — the pre-S17 behaviour for everything.
    Unattributed,
}

/// BUG-341 S3/S17 — restyle root-set (brief §4) for DOM attribute/class/
/// structural changes (chrome `bind_model` diff or a JS DOM mutation).
///
/// Class/attribute/structural selectors don't match ancestors *by themselves*,
/// so the changed node's own subtree covers the node itself and every
/// descendant selector rooted at it (`node X`). What reaches *outside* that
/// subtree is a sibling combinator (`node + X`, `node ~ X`), which the pre-S17
/// version covered by unconditionally invalidating the parent's whole subtree.
///
/// S17 asks the S14 question of that widening: which selectors could react to
/// *this* attribute on *this* node? A `value` write on chrome's omnibox input
/// re-cascaded all 12 elements of `div.omnibox` — and the census found all 12
/// produced a byte-identical `ComputedStyle`, because `chrome.html` has no
/// sibling combinator that could match `#omniInput` (in fact none at all).
/// `index` — from [`restyle_node_index`] — is what answers that per node and
/// per attribute name; a change reported as [`NodeChange::Unattributed`], or a
/// sheet the index declares conservative, keeps the old widen-to-parent
/// behaviour exactly.
///
/// A node with no parent (the document root) invalidates itself either way.
///
/// **Fixed gap (BUG-348/BUG-349):** `:has()` — which this engine does
/// implement (`PseudoClass::Has`, `style.rs`'s `matches_relative`) — lets a
/// change on a node flip some ancestor `E`'s `:has(...)` result, where `E` can
/// sit arbitrarily far above the node's own parent. The parent-only widening
/// below cannot express that reach at any distance beyond one level up, so
/// while [`NodeRestyleIndex::has_has_dependency`] is set (`sheet` contains a
/// `:has()` selector anywhere), every reported change widens to the whole
/// document instead — the conservative fallback the BUG-349 writeup calls for
/// until a real `:has()`-dependency index narrows this further.
///
/// **Known gap, same family:** `:indeterminate` on a radio group and
/// `:default` on a form's submit button read *other* elements' `name`/`checked`/
/// `type` attributes across the whole form. Like `:has()`, that reach is
/// document-shaped rather than subtree-shaped, so neither the pre-S17
/// widen-to-parent nor S17's narrowing expresses it — pre-existing, not
/// introduced here.
pub fn restyle_root_set_for_node_change<'a>(
    doc: &Document,
    changes: impl IntoIterator<Item = (NodeId, NodeChange<'a>)>,
    index: &NodeRestyleIndex<'_>,
) -> HashSet<NodeId> {
    if index.has_has_dependency() {
        return changes.into_iter().map(|_| doc.root()).collect();
    }
    changes
        .into_iter()
        .map(|(n, change)| {
            let needs_fanout = match change {
                NodeChange::Unattributed => true,
                NodeChange::Attr(attr) => index.attr_change_needs_fanout(doc, n, attr),
            };
            if needs_fanout { doc.get(n).parent.unwrap_or(n) } else { n }
        })
        .collect()
}
