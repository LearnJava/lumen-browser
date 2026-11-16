//! PERF-INVALIDATION-S1 — rule-level invalidation sets for the incremental cascade.
//!
//! A flush names what changed on the DOM ([`NodeChange`]s). A rule whose selectors read
//! none of the changed keys cannot flip its match on *any* node in that flush, so a node
//! being recascaded does not have to test it: the previous pass's matched set plus the
//! invalidated rules is a superset of the new matched set, and the matcher/cascade apply
//! unchanged. That is what [`InvalidationIndex`] answers, and what
//! [`crate::counters::RestyleDelta::invalidation`] carries into the walk.
//!
//! Keys are the ones a selector can read off an element (class token, `id`, attribute
//! name — whether written as `[attr]`, as a pseudo-class reading it, or inside a nested
//! selector list) plus the interactive-state pseudo-classes. What cannot be expressed
//! that way is *wide*: `:state()`, `:scope`, `:host`/`::part` (shadow sheets are not
//! scanned), and every structural change ([`NodeChange::ChildList`] and friends) — those
//! always re-test the wide rules and otherwise disable the restriction for the flush.

use std::collections::HashMap;

use lumen_css_parser::{
    ComplexSelector, CompoundSelector, PseudoClass, PseudoElementKind, Rule, SimpleSelector, Stylesheet,
};
use lumen_dom::{Document, NodeId};

use super::restyle::NodeChange;

/// The rule group a [`RuleRef`] points into.
///
/// `@layer` numbering is the *flat* one [`crate::style::CascadeIndex::layers`] indexes by
/// (rules of every `@layer` block concatenated in sheet order), not a block index.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub enum RuleBlock {
    /// `Stylesheet::rules`.
    Top,
    /// Every `@layer` rule; `rule` is the flat position across all blocks.
    Layer,
    /// `Stylesheet::media_rules[block].rules[rule]`.
    Media(u32),
    /// `Stylesheet::supports_rules[block].rules[rule]`.
    Supports(u32),
}

/// One stylesheet rule, identified for the lifetime of one stylesheet revision.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RuleRef {
    /// The block the rule belongs to.
    pub block: RuleBlock,
    /// The rule's position in that block (`@layer`: the flat position, see [`RuleBlock`]).
    pub rule: u32,
}

/// A rule that matched a node in the previous cascade, with the selector that produced the
/// winning specificity — enough to re-apply its declarations without matching it again.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MatchedRule {
    /// The block the rule belongs to.
    pub block: RuleBlock,
    /// The rule's position in that block (`@layer`: the flat position).
    pub rule: u32,
    /// Index into `rule.selectors` of the selector that matched with the highest
    /// specificity; its specificity is what the cascade sorted the declarations under.
    pub selector: u16,
}

impl MatchedRule {
    fn order(&self) -> (RuleBlock, u32) {
        (self.block, self.rule)
    }
}

/// The rules a flush's changes can flip, grouped by block.
///
/// Built by [`InvalidationIndex::invalidated`]. Every block not present has no invalidated
/// rule; rules in [`InvalidationIndex`]'s wide list are present in every answer.
#[derive(Default)]
pub struct InvalidatedRules {
    /// Sorted by block; `rules` sorted ascending; `index` answers the same question the
    /// block's full [`crate::rule_index::RuleIndex`] answers for the whole sheet — "could
    /// this rule match this node at all" — over the invalidated subset only.
    blocks: Vec<InvalidatedBlock>,
    /// Total rules listed, for the census/gate.
    count: u32,
}

struct InvalidatedBlock {
    block: RuleBlock,
    rules: Vec<u32>,
    index: crate::rule_index::RuleIndex,
}

impl InvalidatedRules {
    /// The invalidated rules of `block`, ascending. Empty when the block has none.
    pub fn rules_in(&self, block: RuleBlock) -> &[u32] {
        match self.blocks.binary_search_by(|b| b.block.cmp(&block)) {
            Ok(i) => &self.blocks[i].rules,
            Err(_) => &[],
        }
    }

    /// Whether `rule` of `block` is invalidated.
    pub fn contains(&self, block: RuleBlock, rule: u32) -> bool {
        self.rules_in(block).binary_search(&rule).is_ok()
    }

    /// The invalidated rules of `block` whose rightmost compound could match a node with
    /// these keys — the invalidated subset's [`crate::rule_index::RuleIndex::candidates`].
    pub fn candidates_in<'a>(
        &self,
        block: RuleBlock,
        tag: &str,
        id: Option<&str>,
        classes: impl Iterator<Item = &'a str>,
        attrs: &[lumen_dom::Attribute],
    ) -> Vec<u32> {
        match self.blocks.binary_search_by(|b| b.block.cmp(&block)) {
            Ok(i) => self.blocks[i].index.candidates(tag, id, classes, attrs).into_iter().map(|r| r as u32).collect(),
            Err(_) => Vec::new(),
        }
    }

    /// How many rule refs this set names (duplicates already removed).
    pub fn len(&self) -> u32 {
        self.count
    }

    /// Whether no rule can flip in this flush — a pass may then recompute from the
    /// previous matched set alone.
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    fn from_refs(mut refs: Vec<RuleRef>, sheet: &Stylesheet) -> Self {
        refs.sort_unstable_by_key(|r| (r.block, r.rule));
        refs.dedup();
        let mut blocks: Vec<InvalidatedBlock> = Vec::new();
        for r in refs {
            match blocks.last_mut() {
                Some(b) if b.block == r.block => b.rules.push(r.rule),
                _ => blocks.push(InvalidatedBlock {
                    block: r.block,
                    rules: vec![r.rule],
                    index: crate::rule_index::RuleIndex::empty(),
                }),
            }
        }
        // One index per block present; `flat` positions of `@layer` rules resolve through
        // `sheet.layers` in the same running order `CascadeIndex::layers` uses.
        let mut layer_rules: Vec<&Rule> = Vec::new();
        for layer in &sheet.layers {
            layer_rules.extend(layer.rules.iter());
        }
        for b in &mut blocks {
            let rule_at = |rule: u32| -> Option<&Rule> {
                match b.block {
                    RuleBlock::Top => sheet.rules.get(rule as usize),
                    RuleBlock::Layer => layer_rules.get(rule as usize).copied(),
                    RuleBlock::Media(i) => sheet.media_rules.get(i as usize).and_then(|m| m.rules.get(rule as usize)),
                    RuleBlock::Supports(i) => {
                        sheet.supports_rules.get(i as usize).and_then(|s| s.rules.get(rule as usize))
                    }
                }
            };
            b.index = crate::rule_index::RuleIndex::build_from_indexed(
                b.rules.iter().filter_map(|&rule| rule_at(rule).map(|r| (rule as usize, r))),
            );
        }
        let count = blocks.iter().map(|b| b.rules.len() as u32).sum();
        Self { blocks, count }
    }
}

/// THE ABI of the restricted pass's previous matched set and this flush's invalidations.
///
/// Both are ordered differently on purpose: `prev` follows [`MatchedRule`]'s derived order
/// (block, rule, selector) and is binary-searched; `invalidated.rules_in(block)` answers a
/// block at a time.
pub struct MatchRestriction<'a> {
    /// The rules that matched the node in the previous cascade.
    pub prev: &'a [MatchedRule],
    /// The rules this flush can flip.
    pub invalidated: &'a InvalidatedRules,
}

impl MatchRestriction<'_> {
    /// Whether `block`/`rule` matched in the previous cascade.
    pub(crate) fn was_matched(&self, block: RuleBlock, rule: u32) -> bool {
        self.prev.binary_search_by(|m| m.order().cmp(&(block, rule))).is_ok()
    }
}

/// Keys a stylesheet's selectors read, from a change to the rules that read them.
///
/// Built once per stylesheet revision (the `NodeRestyleIndex` caches it) and interrogated
/// per flush with that flush's [`NodeChange`]s.
#[derive(Default)]
pub struct InvalidationIndex {
    /// ASCII-lowercased class token (quirks compare case-insensitively; lowercasing both
    /// sides only ever invalidates more).
    by_class: HashMap<String, Vec<RuleRef>>,
    by_id: HashMap<String, Vec<RuleRef>>,
    /// ASCII-lowercased attribute name.
    by_attr: HashMap<String, Vec<RuleRef>>,
    /// Interactive-state axes.
    hover: Vec<RuleRef>,
    focus: Vec<RuleRef>,
    active: Vec<RuleRef>,
    /// Rules this model cannot attribute: re-tested on every restricted pass.
    wide: Vec<RuleRef>,
    /// The sheet has `@scope` rules — their matching reads tree position this index does
    /// not model, so no restricted pass is allowed at all.
    pub unmodelled: bool,
}

/// The keys one selector reads, accumulated while scanning a rule.
#[derive(Default)]
struct Keys {
    classes: Vec<String>,
    ids: Vec<String>,
    attrs: Vec<String>,
    hover: bool,
    focus: bool,
    active: bool,
    wide: bool,
}

impl InvalidationIndex {
    /// Scans every rule group the cascade reads (`@scope` and shadow sheets excluded — see
    /// [`Self::unmodelled`] and the module doc).
    pub fn build(sheet: &Stylesheet) -> Self {
        let mut idx = Self { unmodelled: !sheet.scope_rules.is_empty(), ..Self::default() };
        for (i, rule) in sheet.rules.iter().enumerate() {
            idx.add_rule(RuleRef { block: RuleBlock::Top, rule: i as u32 }, rule);
        }
        let mut flat = 0u32;
        for layer in &sheet.layers {
            for rule in &layer.rules {
                idx.add_rule(RuleRef { block: RuleBlock::Layer, rule: flat }, rule);
                flat += 1;
            }
        }
        for (b, media) in sheet.media_rules.iter().enumerate() {
            for (i, rule) in media.rules.iter().enumerate() {
                idx.add_rule(RuleRef { block: RuleBlock::Media(b as u32), rule: i as u32 }, rule);
            }
        }
        for (b, supports) in sheet.supports_rules.iter().enumerate() {
            for (i, rule) in supports.rules.iter().enumerate() {
                idx.add_rule(RuleRef { block: RuleBlock::Supports(b as u32), rule: i as u32 }, rule);
            }
        }
        idx
    }

    fn add_rule(&mut self, r: RuleRef, rule: &Rule) {
        let mut keys = Keys::default();
        for selector in &rule.selectors {
            scan_complex(selector, &mut keys);
        }
        // A wide rule is re-tested on every restricted pass, which subsumes any key lists.
        if keys.wide {
            self.wide.push(r);
            return;
        }
        for c in keys.classes {
            self.by_class.entry(c).or_default().push(r);
        }
        for i in keys.ids {
            self.by_id.entry(i).or_default().push(r);
        }
        for a in keys.attrs {
            self.by_attr.entry(a).or_default().push(r);
        }
        if keys.hover {
            self.hover.push(r);
        }
        if keys.focus {
            self.focus.push(r);
        }
        if keys.active {
            self.active.push(r);
        }
    }

    fn push_attr(&self, name: &str, out: &mut Vec<RuleRef>) {
        if let Some(v) = self.by_attr.get(name) {
            out.extend_from_slice(v);
        }
    }

    /// The rules this flush's `changes` can flip. `None` — the flush is wide: a change names
    /// no expressible key (structural or unattributed), a `class`/`id` write whose old value
    /// is unknown, or a sheet this index does not model.
    ///
    /// `hover`/`focus` are the interactive-state transitions the flush carries; `:active`
    /// rides with `hover` (both are pointer-driven and never flip without it).
    pub fn invalidated(
        &self,
        doc: &Document,
        sheet: &Stylesheet,
        changes: &[(NodeId, NodeChange<'_>)],
        hover: bool,
        focus: bool,
    ) -> Option<InvalidatedRules> {
        if self.unmodelled {
            return None;
        }
        let mut refs: Vec<RuleRef> = Vec::new();
        for &(n, change) in changes {
            match change {
                NodeChange::Unattributed | NodeChange::ChildList | NodeChange::ChildListEnds { .. } => return None,
                NodeChange::Attr(name) => {
                    let name = name.to_ascii_lowercase();
                    // The old value is unknown: any token of `class`/`id` may have changed.
                    if name == "class" || name == "id" {
                        return None;
                    }
                    self.push_attr(&name, &mut refs);
                }
                NodeChange::AttrFrom { name, old } => {
                    let name = name.to_ascii_lowercase();
                    let new = doc.get(n).get_attr(&name).unwrap_or("");
                    match name.as_str() {
                        "class" => {
                            let split = |v: &str| v.split_ascii_whitespace().map(str::to_ascii_lowercase).collect::<Vec<_>>();
                            let (before, after) = (split(old), split(new));
                            for token in before.iter().chain(after.iter()) {
                                if before.contains(token) && after.contains(token) {
                                    continue;
                                }
                                if let Some(v) = self.by_class.get(token) {
                                    refs.extend_from_slice(v);
                                }
                            }
                            self.push_attr("class", &mut refs);
                        }
                        "id" => {
                            for token in [old.to_ascii_lowercase(), new.to_ascii_lowercase()] {
                                if let Some(v) = self.by_id.get(&token) {
                                    refs.extend_from_slice(v);
                                }
                            }
                            self.push_attr("id", &mut refs);
                        }
                        _ => self.push_attr(&name, &mut refs),
                    }
                }
            }
        }
        if hover {
            refs.extend_from_slice(&self.hover);
            // `:active` never flips without a pointer transition; `hover` is that signal.
            refs.extend_from_slice(&self.active);
        }
        if focus {
            refs.extend_from_slice(&self.focus);
        }
        refs.extend_from_slice(&self.wide);
        Some(InvalidatedRules::from_refs(refs, sheet))
    }
}

fn scan_complex(c: &ComplexSelector, keys: &mut Keys) {
    scan_compound(&c.head, keys);
    for (_, compound) in &c.tail {
        scan_compound(compound, keys);
    }
}

fn scan_compound(c: &CompoundSelector, keys: &mut Keys) {
    for part in &c.parts {
        match part {
            SimpleSelector::Class(class) => keys.classes.push(class.to_ascii_lowercase()),
            SimpleSelector::Id(id) => keys.ids.push(id.to_ascii_lowercase()),
            SimpleSelector::Attribute(a) => keys.attrs.push(a.name.to_ascii_lowercase()),
            SimpleSelector::PseudoClass(pc) => scan_pseudo_class(pc, keys),
            // A pseudo-element cannot match an element itself (the element's cascade never
            // applies its rule), but `::slotted()`'s argument names keys a shadow change can
            // read — shadow documents take the wide road anyway, so only its keys matter.
            SimpleSelector::PseudoElement(PseudoElementKind::Slotted(Some(list))) => {
                for c in list {
                    scan_complex(c, keys);
                }
            }
            SimpleSelector::PseudoElement(_) | SimpleSelector::Type(_) | SimpleSelector::Universal => {}
        }
    }
}

fn scan_pseudo_class(pc: &PseudoClass, keys: &mut Keys) {
    match pc {
        PseudoClass::Not(list) | PseudoClass::Is(list) | PseudoClass::Where(list) | PseudoClass::Host(Some(list)) => {
            for c in list {
                scan_complex(c, keys);
            }
        }
        PseudoClass::NthChild(_, Some(list)) | PseudoClass::NthLastChild(_, Some(list)) => {
            for c in list {
                scan_complex(c, keys);
            }
        }
        // The argument matches descendants and later siblings: its keys are what a change
        // below or after the element flips it through (a structural change is wide anyway).
        PseudoClass::Has(rels) => {
            for r in rels {
                scan_complex(&r.selector, keys);
            }
        }
        PseudoClass::State(_) | PseudoClass::Scope | PseudoClass::Host(None) => {
            keys.wide = true;
        }
        PseudoClass::Hover => keys.hover = true,
        PseudoClass::Focus | PseudoClass::FocusWithin | PseudoClass::FocusVisible => keys.focus = true,
        PseudoClass::Active => keys.active = true,
        // Attributes the matcher reads for this pseudo-class; empty for the structural,
        // runtime-constant and interactive ones (which the arms above cover).
        other => {
            for name in super::restyle::pseudo_class_read_attr_names(other) {
                keys.attrs.push(name.to_ascii_lowercase());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_css_parser::parse;

    fn only_element(doc: &Document) -> NodeId {
        (0..doc.node_count() as u32)
            .map(NodeId::from_raw)
            .find(|&n| doc.get(n).get_attr("class").is_some())
            .expect("a classed element")
    }

    fn invalidate_with(css: &str, change: NodeChange<'_>) -> Option<InvalidatedRules> {
        let sheet = parse(css);
        let idx = InvalidationIndex::build(&sheet);
        let doc = lumen_html_parser::parse("<body><p class='a' id='i' data-x='1'></p></body>");
        let node = only_element(&doc);
        idx.invalidated(&doc, &sheet, &[(node, change)], false, false)
    }

    fn refs(css: &str, change: NodeChange<'_>) -> Vec<(RuleBlock, u32)> {
        let got = invalidate_with(css, change).expect("restrictable");
        let mut out = Vec::new();
        for block in [RuleBlock::Top, RuleBlock::Layer, RuleBlock::Media(0)] {
            out.extend(got.rules_in(block).iter().map(|&r| (block, r)));
        }
        out
    }

    #[test]
    fn a_class_change_names_only_the_rules_reading_that_token() {
        let css = ".a { color: red } .b { color: blue } .a .b { margin: 0 } [data-x] { x: y }";
        assert_eq!(
            refs(css, NodeChange::AttrFrom { name: "class", old: "" }),
            vec![(RuleBlock::Top, 0), (RuleBlock::Top, 2)],
            "`.a` and `.a .b` read the added token; `[data-x]` cannot flip on a class write"
        );
    }

    #[test]
    fn an_unknown_class_or_id_write_is_wide() {
        assert!(invalidate_with(".a { color: red }", NodeChange::Attr("class")).is_none());
        assert!(invalidate_with(".a { color: red }", NodeChange::Attr("id")).is_none());
        assert!(invalidate_with(".a { color: red }", NodeChange::Unattributed).is_none());
    }

    #[test]
    fn structural_changes_are_wide() {
        assert!(invalidate_with(":empty { color: red }", NodeChange::ChildList).is_none());
        assert!(
            invalidate_with(":empty { color: red }", NodeChange::ChildListEnds { front: true, back: true, edits: 1 })
                .is_none()
        );
    }

    #[test]
    fn an_attribute_change_reaches_attribute_selectors_and_pseudo_classes_that_read_it() {
        let css = "[data-x] { a: b } :disabled { c: d } .a { e: f }";
        assert_eq!(refs(css, NodeChange::Attr("data-x")), vec![(RuleBlock::Top, 0)]);
        assert_eq!(refs(css, NodeChange::Attr("disabled")), vec![(RuleBlock::Top, 1)]);
        assert_eq!(refs(css, NodeChange::Attr("style")), Vec::new(), "no selector reads `style`");
    }

    #[test]
    fn nested_selectors_and_has_arguments_contribute_keys() {
        let css = ":is(.a, .b) { a: b } .a:has(.c) { d: e } .x:not([data-x]) { f: g }";
        assert_eq!(
            refs(css, NodeChange::AttrFrom { name: "class", old: "" }),
            vec![(RuleBlock::Top, 0), (RuleBlock::Top, 1)],
            "`.b` from `:is`, `.c` from the `:has()` argument"
        );
        assert_eq!(refs(css, NodeChange::Attr("data-x")), vec![(RuleBlock::Top, 2)], "the attribute inside `:not()` still reads it");
    }

    #[test]
    fn unmodellable_rules_stay_wide_and_state_axes_are_opt_in() {
        let css = ".a { a: b } :state(open) { c: d } .h:hover { e: f }";
        // `:state()` is wide, so it rides every answer.
        assert_eq!(refs(css, NodeChange::Attr("data-x")), vec![(RuleBlock::Top, 1)]);
        let sheet = parse(css);
        let idx = InvalidationIndex::build(&sheet);
        let doc = lumen_html_parser::parse("<body><p class='a'></p></body>");
        let node = only_element(&doc);
        let hovered = idx.invalidated(&doc, &sheet, &[(node, NodeChange::Attr("data-x"))], true, false).unwrap();
        assert_eq!(hovered.rules_in(RuleBlock::Top), &[1, 2], "a hover transition names `.h:hover`");
    }

    #[test]
    fn a_sheet_with_scope_rules_refuses_restriction() {
        let css = "@scope (.a) { .b { color: red } }";
        assert!(invalidate_with(css, NodeChange::Attr("data-x")).is_none());
    }
}
