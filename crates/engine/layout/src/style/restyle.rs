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

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use lumen_css_parser::{
    Combinator, ComplexSelector, CompoundSelector, PseudoClass, PseudoElementKind, SimpleSelector, Stylesheet,
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

/// True if `doc` has any shadow root a page attached. Shadow-tree stylesheets
/// ([`SHADOW_SHEETS`]) are not scanned by [`stylesheet_needs_state_fanout`] —
/// modelling their per-host scoping would need the narrowing check to run
/// per-node instead of once per pass, deferred until a fixture demonstrates
/// the benefit is worth that complexity. A document with any such root
/// therefore always takes the conservative widen-to-parent path, matching
/// this engine's pre-S7 behaviour exactly (no regression, just no narrowing
/// win for shadow-DOM-heavy pages yet).
///
/// The UA shadow trees of `<select>`/`<details>`/`<video>`/`<audio>`
/// (GAP-UASHADOWSLOT) do not count: they carry no style sheet, and counting
/// them would switch the narrowing off on nearly every page, Lumen's own
/// chrome included.
fn document_has_shadow_roots(doc: &Document) -> bool {
    doc.has_author_shadow_roots()
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

/// BUG-935 срез 58 — a compound that carries a `:has()`, and whether a flip of
/// its result reaches outside the matched element's own subtree.
struct HasSubject<'a> {
    compound: CompoundRef<'a>,
    /// A sibling combinator follows the compound in its selector (`E:has(x) + F`),
    /// or the `:has()` sits inside a nested selector list whose own combinators
    /// are not analysed: a flip then restyles the parent's subtree, not only `E`'s.
    fanout: bool,
}

/// Visits the argument list of every `:has()` in `compound`: a direct part, or one
/// nested in `:not()`/`:is()`/`:where()`/`:nth-child(… of …)` or in another
/// `:has()`'s argument. `nested` is `false` only for the direct parts.
fn for_each_has_arg<'a>(
    compound: &'a CompoundSelector,
    nested: bool,
    f: &mut impl FnMut(&'a [lumen_css_parser::RelativeSelector], bool),
) {
    for part in &compound.parts {
        let SimpleSelector::PseudoClass(pc) = part else { continue };
        let lists: &[ComplexSelector] = match pc {
            PseudoClass::Not(l) | PseudoClass::Is(l) | PseudoClass::Where(l) => l,
            PseudoClass::NthChild(_, Some(l)) | PseudoClass::NthLastChild(_, Some(l)) => l,
            PseudoClass::Has(rels) => {
                f(rels, nested);
                for r in rels {
                    for_each_has_arg_in_complex(&r.selector, true, f);
                }
                continue;
            }
            _ => continue,
        };
        for c in lists {
            for_each_has_arg_in_complex(c, true, f);
        }
    }
}

fn for_each_has_arg_in_complex<'a>(
    c: &'a ComplexSelector,
    nested: bool,
    f: &mut impl FnMut(&'a [lumen_css_parser::RelativeSelector], bool),
) {
    for_each_has_arg(&c.head, nested, f);
    for (_, comp) in &c.tail {
        for_each_has_arg(comp, nested, f);
    }
}

fn is_sibling_combinator(c: Combinator) -> bool {
    matches!(c, Combinator::NextSibling | Combinator::LaterSibling)
}

/// Whether any `:has()` argument in `complex` looks *forward* along siblings — a
/// leading `+`/`~`, or a sibling combinator inside the argument. Such a `:has()`
/// on `E` can flip when something changes in a sibling *after* `E`, so the
/// elements whose result can flip include the previous siblings of the changed
/// node's ancestors, not only the ancestors.
fn complex_has_sibling_reach(complex: &ComplexSelector) -> bool {
    let mut reach = false;
    for_each_has_arg_in_complex(complex, false, &mut |rels, _| {
        reach |= rels.iter().any(|r| {
            r.combinator.is_some_and(is_sibling_combinator)
                || r.selector.tail.iter().any(|(c, _)| is_sibling_combinator(*c))
        });
    });
    reach
}

/// Registers every compound of `complex` that carries a `:has()` — see [`HasSubject`].
fn collect_has_subjects<'s>(complex: &'s ComplexSelector, out: &mut Vec<(&'s CompoundSelector, bool)>) {
    let mut compounds: Vec<&CompoundSelector> = Vec::with_capacity(1 + complex.tail.len());
    compounds.push(&complex.head);
    let mut combinators: Vec<Combinator> = Vec::with_capacity(complex.tail.len());
    for (comb, comp) in &complex.tail {
        combinators.push(*comb);
        compounds.push(comp);
    }
    for (i, compound) in compounds.iter().enumerate() {
        let mut has_any = false;
        let mut nested_any = false;
        for_each_has_arg(compound, false, &mut |_, nested| {
            has_any = true;
            nested_any |= nested;
        });
        if has_any {
            let fanout = nested_any || combinators[i..].iter().copied().any(is_sibling_combinator);
            out.push((*compound, fanout));
        }
    }
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

/// BUG-935 срез 60 — can `pc`'s result on an element depend on *where the element
/// sits among its siblings* (or on a sibling's/descendant's state)? Those are the
/// pseudo-classes a change of the parent's child list can flip on an element whose
/// own attributes and subtree did not move: the positional family, `:empty`, and
/// `:has()` (a forward-sibling argument reads the following siblings).
/// `:not()`/`:is()`/`:where()` are positional when anything inside them is.
fn pseudo_class_is_positional(pc: &PseudoClass) -> bool {
    match pc {
        PseudoClass::FirstChild
        | PseudoClass::LastChild
        | PseudoClass::OnlyChild
        | PseudoClass::Empty
        | PseudoClass::FirstOfType
        | PseudoClass::LastOfType
        | PseudoClass::OnlyOfType
        | PseudoClass::NthChild(..)
        | PseudoClass::NthLastChild(..)
        | PseudoClass::NthOfType(_)
        | PseudoClass::NthLastOfType(_)
        | PseudoClass::Has(_) => true,
        PseudoClass::Not(list) | PseudoClass::Is(list) | PseudoClass::Where(list) => {
            list.iter().any(complex_selector_is_structure_sensitive)
        }
        _ => false,
    }
}

/// BUG-935 срез 60 — a selector *inside* a nested list (`:not(a + b)`) is treated as
/// structure-sensitive when it has a sibling combinator or any positional compound.
fn complex_selector_is_structure_sensitive(c: &ComplexSelector) -> bool {
    std::iter::once(&c.head).chain(c.tail.iter().map(|(_, comp)| comp)).any(compound_is_positional)
        || c.tail.iter().any(|(comb, _)| is_sibling_combinator(*comb))
}

fn compound_is_positional(compound: &CompoundSelector) -> bool {
    compound.parts.iter().any(|p| matches!(p, SimpleSelector::PseudoClass(pc) if pseudo_class_is_positional(pc)))
}

/// BUG-935 срез 60 — every non-subject compound of `complex` whose match can change
/// when its element's *siblings* change: it is positional, or a sibling combinator
/// precedes it in the selector (`A + B`'s `B`).
///
/// A descendant of such an element can match through it (`li:first-child a`,
/// `h2 + div p`), so when a child list changes, an existing child that could match
/// one of these has to take its whole subtree with it. The subject compound is
/// left out: its element is a child of the changed container (recascaded anyway)
/// or untouched (its own siblings did not change).
fn collect_structure_sensitive_compounds<'a>(complex: &'a ComplexSelector, out: &mut Vec<&'a CompoundSelector>) {
    let subject = complex.tail.len();
    for i in 0..subject {
        let compound = if i == 0 { &complex.head } else { &complex.tail[i - 1].1 };
        let after_sibling = i > 0 && is_sibling_combinator(complex.tail[i - 1].0);
        if after_sibling || compound_is_positional(compound) {
            out.push(compound);
        }
    }
}

/// Whether `node` is an element whose local name is one of `tags` (ASCII case-insensitive).
fn tag_is(doc: &Document, node: NodeId, tags: &[&str]) -> bool {
    matches!(&doc.get(node).data, NodeData::Element { name, .. } if tags.iter().any(|t| name.local.eq_ignore_ascii_case(t)))
}

/// BUG-935 срез 73 — attributes of an element whose only effect is the element's own
/// box or a resource load: no pseudo-class passes them to a descendant, so an attribute
/// selector in an ancestor position (checked by the caller through `AncestorDeps::attrs`)
/// is the only way a write reaches below the element. Deliberately a short list by tag:
/// `<base href>`, `<table cellpadding>`, `<svg><use href>` and the like change what
/// *other* nodes resolve to and stay on the deep path.
fn resource_attr_is_inert(doc: &Document, node: NodeId, attr: &str) -> bool {
    match attr {
        "src" | "srcset" | "sizes" | "alt" | "loading" | "decoding" | "crossorigin" | "referrerpolicy" => {
            tag_is(doc, node, &["img", "source", "iframe", "video", "audio", "track", "embed"])
        }
        "rel" | "target" | "download" | "hreflang" | "ping" => tag_is(doc, node, &["a", "area"]),
        "title" => true,
        _ => false,
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

/// BUG-935 срез 73 — for every class token, `id` and attribute name that some selector reads
/// from an ancestor position ([`AncestorDeps`]), the subject compounds of those selectors.
/// One scan of the sheet answers every write of a flush; keys are ASCII-lowercased.
#[derive(Default)]
struct ReaderTable<'a> {
    classes: HashMap<String, Vec<Arc<CompoundRef<'a>>>>,
    ids: HashMap<String, Vec<Arc<CompoundRef<'a>>>>,
    attrs: HashMap<String, Vec<Arc<CompoundRef<'a>>>>,
}

impl<'a> ReaderTable<'a> {
    fn scan<'s>(sheet: &'s Stylesheet, wrap: impl Fn(&'s CompoundSelector) -> CompoundRef<'a>) -> Self {
        let mut table = Self::default();
        let mut deps = AncestorDeps::default();
        for rules in stylesheet_rule_groups(sheet) {
            for rule in rules {
                for selector in &rule.selectors {
                    // A lone compound reads an ancestor only through a nested selector, and
                    // only a pseudo-class (or `::slotted()`) carries one: most of a real sheet ends here.
                    if selector.tail.is_empty()
                        && !selector.head.parts.iter().any(|p| matches!(p, SimpleSelector::PseudoClass(_) | SimpleSelector::PseudoElement(_)))
                    {
                        continue;
                    }
                    deps.classes.clear();
                    deps.ids.clear();
                    deps.attrs.clear();
                    deps.collect_selector(selector);
                    let subject = Arc::new(wrap(selector.tail.last().map_or(&selector.head, |(_, c)| c)));
                    for (map, keys) in [
                        (&mut table.classes, &deps.classes),
                        (&mut table.ids, &deps.ids),
                        (&mut table.attrs, &deps.attrs),
                    ] {
                        for key in keys {
                            map.entry(key.clone()).or_default().push(Arc::clone(&subject));
                        }
                    }
                }
            }
        }
        table
    }
}

/// BUG-935 срез 73 — subject compounds, bucketed by the one simple selector that must
/// hold for an element to match (`#id`, else a class, else the tag), so that asking
/// "could this element match any of them" costs a few lookups, not one test per compound.
#[derive(Default)]
struct SubjectIndex<'a> {
    by_id: HashMap<&'a str, Vec<&'a CompoundSelector>>,
    by_class: HashMap<&'a str, Vec<&'a CompoundSelector>>,
    by_tag: HashMap<&'a str, Vec<&'a CompoundSelector>>,
    /// No id, class or type in the compound (`[data-x]`, `:not(.a)`, `*`): tried on every element.
    rest: Vec<&'a CompoundSelector>,
}

impl<'a> SubjectIndex<'a> {
    fn insert(&mut self, compound: &'a CompoundSelector) {
        let id = compound.parts.iter().find_map(|p| if let SimpleSelector::Id(i) = p { Some(i.as_str()) } else { None });
        let class =
            compound.parts.iter().find_map(|p| if let SimpleSelector::Class(c) = p { Some(c.as_str()) } else { None });
        let tag =
            compound.parts.iter().find_map(|p| if let SimpleSelector::Type(t) = p { Some(t.as_str()) } else { None });
        match (id, class, tag) {
            (Some(id), ..) => self.by_id.entry(id).or_default().push(compound),
            (None, Some(class), _) => self.by_class.entry(class).or_default().push(compound),
            (None, None, Some(tag)) => self.by_tag.entry(tag).or_default().push(compound),
            (None, None, None) => self.rest.push(compound),
        }
    }

    /// Whether some indexed compound could match the element, pseudo-classes and
    /// pseudo-elements counted as matching (the answer only ever widens).
    fn could_match(&self, doc: &Document, node: NodeId, tag: &str, attrs: &[lumen_dom::Attribute]) -> bool {
        let fits = |c: &&CompoundSelector| {
            c.parts.iter().all(|part| match part {
                SimpleSelector::PseudoElement(_) | SimpleSelector::PseudoClass(_) => true,
                other => matches_simple(other, doc, node, tag, attrs),
            })
        };
        let value = |wanted: &str| attrs.iter().find(|a| a.name.local == wanted).map(|a| a.value.as_str());
        value("id").and_then(|id| self.by_id.get(id)).is_some_and(|v| v.iter().any(fits))
            || value("class").is_some_and(|classes| {
                classes.split_whitespace().any(|c| self.by_class.get(c).is_some_and(|v| v.iter().any(fits)))
            })
            || self.by_tag.get(tag).is_some_and(|v| v.iter().any(fits))
            || self.rest.iter().any(fits)
    }
}

/// BUG-935 срез 68 — what the sheet's selectors read from an element's *ancestors*, by
/// class token, `id` and attribute name.
///
/// A write to `R`'s attribute reaches `R`'s descendants only through a selector that has
/// `R` in a non-subject position (`.open .item`, `[data-x] > a`, `:not(.a) b`) — the
/// subject compound is matched against the descendant itself. So when the tokens a write
/// changed appear in none of those compounds, no descendant's match can have flipped, and
/// the root needs `R` and its direct children rather than its whole subtree
/// ([`NodeRestyleIndex::attr_change_stays_local`]).
#[derive(Default)]
struct AncestorDeps {
    /// Class tokens (ASCII-lowercased: quirks mode compares them case-insensitively).
    classes: HashSet<String>,
    ids: HashSet<String>,
    /// Lowercased attribute names keyed by an attribute selector.
    attrs: HashSet<String>,
    /// An ancestor-position compound carries a pseudo-class that reads a link's `href`
    /// (`a:link > span`): a write to `href` then reaches descendants.
    link_state: bool,
    /// The sheet holds something this scan does not model (`@scope`: its root/limit are
    /// stored as text and matched apart from the rules' selectors).
    unmodelled: bool,
}

impl AncestorDeps {
    /// `compound` is matched against an element; `subject` — that element is the one a
    /// rule styles, not an ancestor of it. Everything inside a non-subject compound
    /// counts; in a subject one only the nested selectors that walk up the tree do.
    fn collect_compound(&mut self, compound: &CompoundSelector, subject: bool) {
        for part in &compound.parts {
            match part {
                SimpleSelector::Class(c) if !subject => {
                    self.classes.insert(c.to_ascii_lowercase());
                }
                SimpleSelector::Id(i) if !subject => {
                    self.ids.insert(i.to_ascii_lowercase());
                }
                SimpleSelector::Attribute(a) if !subject => {
                    self.attrs.insert(a.name.to_ascii_lowercase());
                }
                SimpleSelector::PseudoClass(pc) => self.collect_pseudo_class(pc, subject),
                SimpleSelector::PseudoElement(PseudoElementKind::Slotted(Some(list))) => {
                    for c in list {
                        self.collect_complex(c, false);
                    }
                }
                _ => {}
            }
        }
    }

    fn collect_pseudo_class(&mut self, pc: &PseudoClass, subject: bool) {
        match pc {
            PseudoClass::Not(list)
            | PseudoClass::Is(list)
            | PseudoClass::Where(list)
            | PseudoClass::NthChild(_, Some(list))
            | PseudoClass::NthLastChild(_, Some(list))
            | PseudoClass::Host(Some(list)) => {
                for c in list {
                    // A nested selector without combinators is still matched against the
                    // same element as the compound that carries it.
                    self.collect_complex(c, subject && c.tail.is_empty());
                }
            }
            // `:has()` reads the matched element's descendants and later siblings: an
            // ancestor of it is never inside its argument.
            PseudoClass::Has(rels) if !subject => {
                for r in rels {
                    self.collect_complex(&r.selector, false);
                }
            }
            // `id` against the URL fragment: the *value* of `id` is what flips the match,
            // and no `#id` part names it.
            PseudoClass::Target | PseudoClass::TargetWithin => {
                self.attrs.insert("id".to_string());
            }
            // Match on the element's own `href` (and, for `:current`/`:past`/`:future`, on
            // the document's links): only a compound that is *not* the subject can pass
            // that to a descendant.
            PseudoClass::Link
            | PseudoClass::Visited
            | PseudoClass::AnyLink
            | PseudoClass::Current
            | PseudoClass::Past
            | PseudoClass::Future
                if !subject =>
            {
                self.link_state = true;
            }
            _ => {}
        }
    }

    fn collect_complex(&mut self, complex: &ComplexSelector, subject: bool) {
        self.collect_compound(&complex.head, subject);
        for (_, compound) in &complex.tail {
            self.collect_compound(compound, subject);
        }
    }

    /// Registers everything in `complex` — and in any selector nested in it — that has a
    /// sibling combinator: a write to an element's attribute reaches that element's siblings
    /// through the compounds on the left of such a combinator. Over-approximates by taking the
    /// whole selector, subject included.
    fn collect_sibling_reach(&mut self, complex: &ComplexSelector) {
        if complex.tail.iter().any(|(c, _)| is_sibling_combinator(*c)) {
            self.collect_complex(complex, false);
        }
        for compound in std::iter::once(&complex.head).chain(complex.tail.iter().map(|(_, c)| c)) {
            for part in &compound.parts {
                match part {
                    SimpleSelector::PseudoClass(
                        PseudoClass::Not(list)
                        | PseudoClass::Is(list)
                        | PseudoClass::Where(list)
                        | PseudoClass::NthChild(_, Some(list))
                        | PseudoClass::NthLastChild(_, Some(list))
                        | PseudoClass::Host(Some(list)),
                    ) => list.iter().for_each(|c| self.collect_sibling_reach(c)),
                    SimpleSelector::PseudoClass(PseudoClass::Has(rels)) => {
                        for r in rels {
                            if r.combinator.is_some_and(is_sibling_combinator) {
                                self.collect_complex(&r.selector, false);
                            }
                            self.collect_sibling_reach(&r.selector);
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    /// Whether a write that moved `old` to `new` on a `class` (or `id`) attribute touches
    /// anything this scan holds: a token on one side only that is listed, or an attribute
    /// selector keyed on the attribute itself (whose match reads the whole value).
    fn class_write_hits(&self, old: &str, new: &str) -> bool {
        if self.attrs.contains("class") {
            return true;
        }
        let in_set = |token: &str| self.classes.contains(&token.to_ascii_lowercase());
        old.split_ascii_whitespace().filter(|t| !new.split_ascii_whitespace().any(|n| n == *t)).any(in_set)
            || new.split_ascii_whitespace().filter(|t| !old.split_ascii_whitespace().any(|o| o == *t)).any(in_set)
    }

    fn id_write_hits(&self, old: &str, new: &str) -> bool {
        old != new
            && (self.attrs.contains("id")
                || self.ids.contains(&old.to_ascii_lowercase())
                || self.ids.contains(&new.to_ascii_lowercase()))
    }

    /// Registers `complex`: every compound but the last is an ancestor (or a sibling of
    /// one) of the element the rule styles.
    fn collect_selector(&mut self, complex: &ComplexSelector) {
        let subject = complex.tail.len();
        for i in 0..=subject {
            let compound = if i == 0 { &complex.head } else { &complex.tail[i - 1].1 };
            self.collect_compound(compound, i == subject);
        }
    }
}

/// BUG-341 S17 — what [`restyle_root_set_for_node_change`] needs to know about
/// the stylesheet in play, computed once per layout pass.
///
/// The DOM-mutation counterpart of [`StateRestyleIndex`], and built from the
/// same single scan over every rule list in the sheet.
pub struct NodeRestyleIndex<'a> {
    /// The sheet the index was built from — [`Self::affected_descendants`] scans its
    /// selectors once ([`ReaderTable`]), for the first write it is asked about.
    sheet: SheetRef<'a>,
    /// BUG-935 срез 73 — [`ReaderTable`] of `sheet`, built on the first write that asks.
    readers: std::cell::OnceCell<ReaderTable<'a>>,
    /// Every compound in `sheet` from which a sibling combinator is reachable
    /// ([`collect_sibling_source_compounds`]).
    sibling_sources: Vec<CompoundRef<'a>>,
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
    /// BUG-935 срез 58 — every compound in `sheet` that carries a `:has()`. A
    /// change on a node can flip the result only on an element that could match
    /// one of these ([`Self::has_reach_roots`]), which is what lets the root-set
    /// name those ancestors instead of the whole document.
    has_subjects: Vec<HasSubject<'a>>,
    /// Some `:has()` argument looks forward along siblings, so the previous
    /// siblings of the changed node's ancestors can flip too.
    has_sibling_reach: bool,
    /// The document has an author shadow root. Shadow-tree sheets are not scanned
    /// and `:has()` does not cross the boundary, so with a `:has()` in the sheet
    /// the whole document is restyled, as before.
    has_in_shadow_doc: bool,
    /// BUG-935 срез 60 — the non-subject compounds a child-list change can flip
    /// ([`collect_structure_sensitive_compounds`]).
    structure_sensitive: Vec<CompoundRef<'a>>,
    /// BUG-935 срез 68 — what the selectors read from ancestors ([`AncestorDeps`]).
    ancestor_deps: AncestorDeps,
    /// BUG-935 срез 68 — what the selectors with a sibling combinator read from an element
    /// that has siblings after it ([`AncestorDeps::collect_sibling_reach`]).
    sibling_deps: AncestorDeps,
    /// BUG-935 срез 68 — [`Self::attr_change_stays_local`] may answer `true`. Off only through
    /// [`Self::set_attr_narrowing`]: the A/B switch of a live measurement and the baseline
    /// of the differential tests.
    attr_narrowing: bool,
}

/// Indexes the subject compounds of `list` and counts them into `readers`.
fn add_readers<'t>(
    subjects: &mut SubjectIndex<'t>,
    readers: &mut usize,
    list: Option<&'t Vec<Arc<CompoundRef<'_>>>>,
) {
    for c in list.into_iter().flatten() {
        subjects.insert(c);
        *readers += 1;
    }
}

impl<'a> NodeRestyleIndex<'a> {
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

    /// BUG-935 срез 58 — the elements whose `:has()` result a change on `node` can
    /// flip, as restyle roots, added to `out`.
    ///
    /// `E:has(rel)` reads `E`'s descendants (and, for `+`/`~`, its following
    /// siblings), so a change on `node` reaches `E` only when `E` is `node` or one
    /// of its ancestors — or, with a forward-sibling argument, a previous sibling of
    /// one of those. Of those, only an element that could match a compound carrying
    /// a `:has()` can have a result to flip, which is decided structurally: its other
    /// parts must match, while every pseudo-class is taken as possible.
    fn has_reach_roots(&self, doc: &Document, node: NodeId, out: &mut HashSet<NodeId>) {
        let mut consider = |a: NodeId| {
            for subject in &self.has_subjects {
                if compound_could_match_after_attr_change(&subject.compound, doc, a, "") {
                    out.insert(if subject.fanout { doc.get(a).parent.unwrap_or(a) } else { a });
                }
            }
        };
        let mut cur = Some(node);
        while let Some(a) = cur {
            consider(a);
            let parent = doc.get(a).parent;
            if self.has_sibling_reach
                && let Some(p) = parent
            {
                for &sib in doc.get(p).children.iter().take_while(|&&c| c != a) {
                    consider(sib);
                }
            }
            cur = parent;
        }
    }

    /// BUG-935 срез 60 — can a change of the parent's child list alter the style of
    /// something *below* `child`, through `child`'s own position or siblings
    /// (`li:first-child a`, `h2 + div p`)? Decided structurally, over-approximating:
    /// every pseudo-class is taken as possible ([`compound_could_match_after_attr_change`]).
    fn child_needs_deep_restyle(&self, doc: &Document, child: NodeId) -> bool {
        self.structure_sensitive
            .iter()
            .any(|c| compound_could_match_after_attr_change(c, doc, child, ""))
    }

    /// BUG-935 срез 68 — [`Self::attr_change_needs_fanout`] for a write whose old value may be
    /// known. A `class`/`id` write that moved only tokens no sibling-combinator selector
    /// mentions cannot flip any such selector's match on `node`, so it reaches no sibling
    /// (`.side + .card` is silent about a `loaded` class); the structural question is asked
    /// only when it could.
    pub fn attr_change_needs_fanout_from(&self, doc: &Document, node: NodeId, attr: &str, old: Option<&str>) -> bool {
        if self.conservative {
            return true;
        }
        if !self.attr_narrowing {
            return self.attr_change_needs_fanout(doc, node, attr);
        }
        if let (Some(old), NodeData::Element { attrs, .. }) = (old, &doc.get(node).data) {
            let current = |wanted: &str| {
                attrs.iter().find(|a| a.name.local.eq_ignore_ascii_case(wanted)).map_or("", |a| a.value.as_str())
            };
            let silent = if attr.eq_ignore_ascii_case("class") {
                !self.sibling_deps.class_write_hits(old, current("class"))
            } else if attr.eq_ignore_ascii_case("id") {
                !self.sibling_deps.id_write_hits(old, current("id"))
            } else {
                false
            };
            if silent {
                return false;
            }
        }
        self.attr_change_needs_fanout(doc, node, attr)
    }

    /// BUG-935 срез 68 — can a write to `node`'s `attr` change the style of anything
    /// *below* `node`, i.e. does some selector read it from an ancestor position?
    ///
    /// `old` is the attribute's value before the write when the caller knows it
    /// (`None` = unknown, never narrowed for `class`/`id`, whose changed tokens it is
    /// the value that names). Sound only for the attributes whose effect on a
    /// descendant is *entirely* a selector match — `class`, `id`, `style` (own
    /// declarations; descendants see them through inheritance, which the cascade's own
    /// style comparison catches), `data-*` and `aria-*`. `lang`, `dir`, `disabled`,
    /// `hidden`… reach descendants through pseudo-classes (`:lang()`, `:dir()`,
    /// `:disabled`) and stay on the deep path. Sibling reach is a separate question
    /// ([`Self::attr_change_needs_fanout`]) that the caller asks first.
    pub fn attr_change_stays_local(&self, doc: &Document, node: NodeId, attr: &str, old: Option<&str>) -> bool {
        let deps = &self.ancestor_deps;
        if !self.attr_narrowing || self.conservative || deps.unmodelled {
            return false;
        }
        let name = attr.to_ascii_lowercase();
        if deps.attrs.contains(&name) {
            return false;
        }
        let NodeData::Element { attrs, .. } = &doc.get(node).data else {
            return false;
        };
        let current = |wanted: &str| {
            attrs.iter().find(|a| a.name.local.eq_ignore_ascii_case(wanted)).map_or("", |a| a.value.as_str())
        };
        match name.as_str() {
            "class" => old.is_some_and(|old| !deps.class_write_hits(old, current("class"))),
            "id" => old.is_some_and(|old| !deps.id_write_hits(old, current("id"))),
            "style" => true,
            "href" => !deps.link_state && tag_is(doc, node, &["a", "area"]),
            n => n.starts_with("data-") || n.starts_with("aria-") || resource_attr_is_inert(doc, node, n),
        }
    }

    /// BUG-935 срез 73 — the descendants of `node` whose style a write of `attr` on `node`
    /// can change, for a write that [`Self::attr_change_stays_local`] refused because
    /// some selector does read it from an ancestor position (`html.js .menu`).
    ///
    /// Such a selector styles only elements its *subject* compound matches. So the
    /// descendants a write can reach are those that match the subject compound of one of
    /// the selectors that read the written token — with every pseudo-class and
    /// pseudo-element in it taken as matching, which only widens the set. Everything else
    /// under `node` keeps the set of rules it matched, hence its style, as long as the
    /// style it inherits does not move (the cascade's own comparison of `node`'s style
    /// follows that). `None` — the write is not one this analysis models (`class` and
    /// `id` need the old value; `data-*`/`aria-*` go by name; `@scope` in the sheet), or
    /// the selectors that read it are too many to be worth indexing: the caller takes the
    /// deep path.
    pub fn affected_descendants(
        &self,
        doc: &Document,
        node: NodeId,
        attr: &str,
        old: Option<&str>,
    ) -> Option<Vec<NodeId>> {
        /// More selectors than this reading one token and the subtree walk below is no
        /// longer cheaper than recascading it.
        const MAX_READERS: usize = 4096;
        if !self.attr_narrowing || self.conservative || self.ancestor_deps.unmodelled {
            return None;
        }
        let NodeData::Element { attrs, .. } = &doc.get(node).data else {
            return None;
        };
        let name = attr.to_ascii_lowercase();
        let current = |wanted: &str| {
            attrs.iter().find(|a| a.name.local.eq_ignore_ascii_case(wanted)).map_or("", |a| a.value.as_str())
        };
        let table = self.readers.get_or_init(|| match &self.sheet {
            SheetRef::Borrowed(sheet) => ReaderTable::scan(sheet, CompoundRef::Borrowed),
            SheetRef::Shared(sheet) => ReaderTable::scan(sheet, CompoundRef::owned),
        });
        let mut subjects = SubjectIndex::default();
        let mut readers = 0usize;
        match name.as_str() {
            "class" => {
                let (old, new) = (old?, current("class"));
                let split = |v: &str| v.split_ascii_whitespace().map(str::to_ascii_lowercase).collect::<HashSet<_>>();
                let (before, after) = (split(old), split(new));
                for token in before.symmetric_difference(&after) {
                    add_readers(&mut subjects, &mut readers, table.classes.get(token));
                }
                add_readers(&mut subjects, &mut readers, table.attrs.get("class"));
            }
            "id" => {
                let (old, new) = (old?, current("id"));
                if old != new {
                    add_readers(&mut subjects, &mut readers, table.ids.get(&old.to_ascii_lowercase()));
                    add_readers(&mut subjects, &mut readers, table.ids.get(&new.to_ascii_lowercase()));
                }
                add_readers(&mut subjects, &mut readers, table.attrs.get("id"));
            }
            n if n.starts_with("data-") || n.starts_with("aria-") => {
                add_readers(&mut subjects, &mut readers, table.attrs.get(n));
            }
            _ => return None,
        }
        if readers > MAX_READERS {
            return None;
        }
        let mut out = Vec::new();
        let mut stack: Vec<NodeId> = doc.get(node).children.iter().rev().copied().collect();
        while let Some(id) = stack.pop() {
            let n = doc.get(id);
            if let NodeData::Element { name, attrs } = &n.data {
                if subjects.could_match(doc, id, &name.local, attrs) {
                    out.push(id);
                }
                stack.extend(n.children.iter().rev().copied());
            }
        }
        Some(out)
    }

    /// BUG-935 срез 68 — turns the attribute narrowing ([`Self::attr_change_stays_local`])
    /// on or off for this index; on by default.
    pub fn set_attr_narrowing(&mut self, on: bool) {
        self.attr_narrowing = on;
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
    build_node_index(doc, sheet, SheetRef::Borrowed(sheet), CompoundRef::Borrowed)
}

/// BUG-935 срез 74 — [`restyle_node_index`] over a shared sheet: the index holds the `Arc`
/// instead of a borrow, so a caller can keep it for as long as the sheet's revision stays
/// the same (`doc` matters only through whether it has an author shadow root).
pub fn restyle_node_index_shared(doc: &Document, sheet: &Arc<Stylesheet>) -> NodeRestyleIndex<'static> {
    build_node_index(doc, sheet, SheetRef::Shared(Arc::clone(sheet)), CompoundRef::owned)
}

/// Where [`NodeRestyleIndex`] reads its sheet from when it needs to scan it again.
enum SheetRef<'a> {
    Borrowed(&'a Stylesheet),
    Shared(Arc<Stylesheet>),
}

/// A compound of the sheet an index was built for: borrowed from it, or — for an index that has
/// to outlive any borrow of the sheet — a copy.
enum CompoundRef<'a> {
    Borrowed(&'a CompoundSelector),
    Owned(Box<CompoundSelector>),
}

impl CompoundRef<'_> {
    fn owned(compound: &CompoundSelector) -> CompoundRef<'static> {
        CompoundRef::Owned(Box::new(compound.clone()))
    }
}

impl std::ops::Deref for CompoundRef<'_> {
    type Target = CompoundSelector;

    fn deref(&self) -> &CompoundSelector {
        match self {
            CompoundRef::Borrowed(c) => c,
            CompoundRef::Owned(c) => c,
        }
    }
}

fn build_node_index<'a, 's>(
    doc: &Document,
    sheet: &'s Stylesheet,
    sheet_ref: SheetRef<'a>,
    wrap: impl Fn(&'s CompoundSelector) -> CompoundRef<'a>,
) -> NodeRestyleIndex<'a> {
    let has_in_shadow_doc = document_has_shadow_roots(doc);
    let mut conservative = has_in_shadow_doc;
    let mut has_dependent = false;
    let mut has_sibling_reach = false;
    let mut sibling_sources: Vec<&CompoundSelector> = Vec::new();
    let mut has_subjects: Vec<(&CompoundSelector, bool)> = Vec::new();
    let mut structure_sensitive: Vec<&CompoundSelector> = Vec::new();
    let mut ancestor_deps = AncestorDeps { unmodelled: !sheet.scope_rules.is_empty(), ..AncestorDeps::default() };
    let mut sibling_deps = AncestorDeps::default();
    for rules in stylesheet_rule_groups(sheet) {
        for rule in rules {
            for selector in &rule.selectors {
                ancestor_deps.collect_selector(selector);
                sibling_deps.collect_sibling_reach(selector);
                if complex_selector_has_any_has(selector) {
                    has_dependent = true;
                    has_sibling_reach |= complex_has_sibling_reach(selector);
                    collect_has_subjects(selector, &mut has_subjects);
                }
                conservative |= complex_selector_has_nth_of(selector);
                collect_sibling_source_compounds(selector, &mut sibling_sources);
                collect_structure_sensitive_compounds(selector, &mut structure_sensitive);
            }
        }
    }
    NodeRestyleIndex {
        sheet: sheet_ref,
        readers: std::cell::OnceCell::new(),
        sibling_sources: sibling_sources.into_iter().map(&wrap).collect(),
        conservative,
        has_dependent,
        has_subjects: has_subjects
            .into_iter()
            .map(|(compound, fanout)| HasSubject { compound: wrap(compound), fanout })
            .collect(),
        has_sibling_reach,
        has_in_shadow_doc,
        structure_sensitive: structure_sensitive.into_iter().map(&wrap).collect(),
        ancestor_deps,
        sibling_deps,
        attr_narrowing: true,
    }
}

/// BUG-341 S17 — one reported DOM mutation, as
/// [`restyle_root_set_for_node_change`] needs to see it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeChange<'a> {
    /// The attribute named `.0` was written to, or removed from, the node. The
    /// name is what lets the root-set ask which selectors could possibly react.
    Attr(&'a str),
    /// BUG-935 срез 68 — like [`Self::Attr`], and the value the attribute had *before*
    /// the write (`""` when it was absent), which is what lets the root-set name the
    /// class/id tokens that changed. Only a source that knows the old value reports it.
    AttrFrom {
        /// The attribute written.
        name: &'a str,
        /// Its value at the start of the interval the change covers.
        old: &'a str,
    },
    /// BUG-935 срез 60 — the node's child list changed (an element was appended,
    /// removed, moved, or its text replaced) and nothing else about it did.
    /// [`restyle_roots_for_node_changes`] can restyle the node and its direct
    /// children instead of the parent's whole subtree; the single-set
    /// [`restyle_root_set_for_node_change`] reads it as [`Self::Unattributed`].
    ChildList,
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
/// below cannot express that reach, so while [`NodeRestyleIndex::has_has_dependency`]
/// is set (`sheet` contains a `:has()` selector anywhere) each change also adds
/// the ancestors (and, for forward-sibling arguments, their previous siblings)
/// that could match a compound carrying a `:has()` — see
/// [`NodeRestyleIndex::has_reach_roots`]. BUG-349 first widened every change to
/// the whole document; BUG-935 s58 replaced that with this index, because on a
/// real page every forced reflow restyled and re-collected all of it. A
/// document with an author shadow root (shadow sheets are not scanned) still
/// widens to the whole document.
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
    root_set_impl(doc, changes, index, false).deep
}

/// BUG-935 срез 60 — the restyle roots of a batch of DOM changes, split by how much of
/// the subtree each one has to take with it.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RestyleRoots {
    /// Nodes whose whole subtree is recascaded — [`restyle_root_set_for_node_change`]'s
    /// answer, and what `RestyleDelta::dirty_roots` means.
    pub deep: HashSet<NodeId>,
    /// Nodes whose child list changed: the node and its direct children are
    /// recascaded, and a child's subtree only when its style changed, it moved from
    /// another parent, or it is in [`Self::deep`] (`RestyleDelta::shallow_roots`).
    pub shallow: HashSet<NodeId>,
    /// BUG-935 срез 73 — descendants of a [`Self::shallow`] root that a selector reads the
    /// root's written token for: each is recascaded on its own (`RestyleDelta::point_roots`),
    /// the rest of the shallow root's subtree is left alone.
    pub point: HashSet<NodeId>,
}

/// BUG-935 срез 60 — like [`restyle_root_set_for_node_change`], but a
/// [`NodeChange::ChildList`] yields a shallow root instead of the parent's whole subtree.
///
/// A child-list change on `C` reaches only: `C` itself (`:empty`, `:has()`), its direct
/// children (positional pseudo-classes, sibling combinators), and the subtree of a child
/// whose own position a selector reads on the way down (`li:first-child a` —
/// [`NodeRestyleIndex::child_needs_deep_restyle`], which lands such a child in `deep`).
/// What goes beyond that is a sibling combinator on `C` (`C:empty + X`), answered by also
/// making `C`'s parent a shallow root, and `:has()`, answered by the same
/// [`NodeRestyleIndex::has_reach_roots`] as before. `:nth-child(… of S)` and shadow roots
/// switch it off (`conservative`): the change then widens to the parent, as before.
pub fn restyle_roots_for_node_changes<'a>(
    doc: &Document,
    changes: impl IntoIterator<Item = (NodeId, NodeChange<'a>)>,
    index: &NodeRestyleIndex<'_>,
) -> RestyleRoots {
    root_set_impl(doc, changes, index, true)
}

fn root_set_impl<'a>(
    doc: &Document,
    changes: impl IntoIterator<Item = (NodeId, NodeChange<'a>)>,
    index: &NodeRestyleIndex<'_>,
    allow_shallow: bool,
) -> RestyleRoots {
    if index.has_dependent && index.has_in_shadow_doc {
        return RestyleRoots { deep: changes.into_iter().map(|_| doc.root()).collect(), ..RestyleRoots::default() };
    }
    let mut roots = RestyleRoots::default();
    let shallow_ok = allow_shallow && !index.conservative;
    for (n, change) in changes {
        match change {
            NodeChange::ChildList if shallow_ok => {
                // A text/comment node reported for a data change: its parent's child
                // list is what `:empty` and the siblings read.
                let container = if matches!(doc.get(n).data, NodeData::Element { .. }) {
                    Some(n)
                } else {
                    doc.get(n).parent
                };
                let Some(n) = container else {
                    roots.deep.insert(n);
                    continue;
                };
                let mut containers = vec![n];
                if index.attr_change_needs_fanout(doc, n, "")
                    && let Some(parent) = doc.get(n).parent
                {
                    containers.push(parent);
                }
                for c in containers {
                    if roots.shallow.insert(c) {
                        for &child in &doc.get(c).children {
                            if matches!(doc.get(child).data, NodeData::Element { .. })
                                && index.child_needs_deep_restyle(doc, child)
                            {
                                roots.deep.insert(child);
                            }
                        }
                    }
                }
            }
            other => {
                let (needs_fanout, written) = match other {
                    NodeChange::Attr(attr) => (index.attr_change_needs_fanout(doc, n, attr), Some((attr, None))),
                    NodeChange::AttrFrom { name, old } => {
                        (index.attr_change_needs_fanout_from(doc, n, name, Some(old)), Some((name, Some(old))))
                    }
                    NodeChange::Unattributed | NodeChange::ChildList => (true, None),
                };
                // BUG-935 срез 68: nothing below `n` reads what was written, so `n` and its
                // direct children are restyled and the walk goes deeper only if `n`'s style
                // moved — the shallow-root contract, with the attribute (not the child
                // list) as the reason.
                let local = shallow_ok
                    && !needs_fanout
                    && written.is_some_and(|(name, old)| index.attr_change_stays_local(doc, n, name, old));
                // BUG-935 срез 73: a write some selector *does* read from an ancestor position
                // names the descendants that selector can style; those, and not the whole
                // subtree, are what the cascade has to look at.
                let affected = if local || !shallow_ok || needs_fanout {
                    None
                } else {
                    written.and_then(|(name, old)| index.affected_descendants(doc, n, name, old))
                };
                if local {
                    roots.shallow.insert(n);
                } else if let Some(points) = affected {
                    roots.shallow.insert(n);
                    roots.point.extend(points);
                } else {
                    roots.deep.insert(if needs_fanout { doc.get(n).parent.unwrap_or(n) } else { n });
                }
            }
        }
        if index.has_dependent {
            index.has_reach_roots(doc, n, &mut roots.deep);
        }
    }
    roots
}
