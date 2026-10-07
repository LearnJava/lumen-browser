//! BUG-935 срез 98 — a custom property written on an element the cascade restyles *shallowly*
//! (`body.style.setProperty('--scrollbar-compensation', …)`, a `--scroll` a page script moves every
//! frame) used to re-cascade its whole subtree: custom properties inherit, so every descendant's
//! style differs from the previous pass in `custom_props` and the level-by-level descent of
//! срез 92 never stopped (rbc.ru: 850–940 elements, ~90 ms of the walk).
//!
//! A descendant's style is a function of the rules that match it and of its parent's style. When
//! the parent's style changed in `custom_props` *only*, and no rule that could apply to the
//! descendant reads or declares a changed name, the new style is the old one with the parent's
//! new values for the changed names — no matching, no cascade. [`CustomDep`] is the sheet-side
//! half: which names can be affected and which selectors could reach an affected element.
//!
//! Everything here over-approximates: a rule is "dependent" if its declaration text merely
//! *mentions* a name next to a `var(`, selectors of every container are tried whether the
//! container applies or not, and any construct that can read a custom property in a way the
//! text scan cannot see (`@property`, `@function`, `@mixin`, `@container`, `@starting-style`,
//! `var()` in `@keyframes`, a shadow tree) turns the whole mechanism off.

use std::rc::Rc;

use lumen_css_parser::{ComplexSelector, Declaration, Rule, Stylesheet};
use lumen_dom::{Document, NodeId};

use crate::style::{matches_complex, ComputedStyle, CustomProps};

/// More dependent selectors than this and testing every element against them costs about what
/// the cascade does.
const MAX_SELECTORS: usize = 512;

/// `LUMEN_NO_CUSTOM_FLOW=1` — every descendant of a restyled element goes through the cascade
/// again, as before срез 98. A/B switch for a live measurement. Read once per process.
fn disabled() -> bool {
    static OFF: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *OFF.get_or_init(|| std::env::var_os("LUMEN_NO_CUSTOM_FLOW").is_some_and(|v| v != "0"))
}

/// What a change of custom properties can reach in one sheet.
pub(crate) struct CustomDep<'s> {
    /// The changed names and every custom property whose value can follow from them.
    names: Vec<String>,
    /// The selectors of every rule that reads or declares a changed name, directly or through a
    /// custom property whose value reads one.
    selectors: Vec<&'s ComplexSelector>,
}

/// The change one element's style carries down to its children, with the sheet-side analysis.
pub(crate) struct CustomFlow<'s> {
    dep: Rc<CustomDep<'s>>,
    /// The names whose value differs between the element that started the flow and its previous
    /// style — the only entries of a descendant's `custom_props` the flow rewrites.
    changed: Rc<Vec<String>>,
    /// The element's `custom_props` before and after this pass.
    old: CustomProps,
    new: CustomProps,
}

/// Every rule of every container [`Stylesheet`] keeps its style rules in.
fn all_rules(sheet: &Stylesheet) -> impl Iterator<Item = &Rule> {
    sheet
        .rules
        .iter()
        .chain(sheet.media_rules.iter().flat_map(|m| m.rules.iter()))
        .chain(sheet.supports_rules.iter().flat_map(|s| s.rules.iter()))
        .chain(sheet.layers.iter().flat_map(|l| l.rules.iter()))
        .chain(sheet.scope_rules.iter().flat_map(|s| s.rules.iter()))
}

/// Whether `decl`'s value reads one of `names` (over-approximation: a `var(` and the name's text).
fn reads_any(decl: &Declaration, names: &[String]) -> bool {
    decl.value.contains("var(") && names.iter().any(|n| decl.value.contains(n.as_str()))
}

impl<'s> CustomDep<'s> {
    /// `None` — the sheet has something the analysis cannot see through.
    fn build(sheet: &'s Stylesheet, changed: Vec<String>) -> Option<Self> {
        let opaque = !sheet.properties.is_empty()
            || !sheet.function_rules.is_empty()
            || !sheet.mixin_rules.is_empty()
            || !sheet.container_rules.is_empty()
            || !sheet.starting_style_rules.is_empty()
            || sheet
                .keyframes
                .iter()
                .any(|k| k.frames.iter().any(|f| f.declarations.iter().any(|d| d.value.contains("var("))));
        if opaque {
            return None;
        }
        // Names whose value can change: the changed ones and, to a fixpoint, every custom
        // property declared with a value that reads one of them.
        let mut names = changed;
        loop {
            let before = names.len();
            for rule in all_rules(sheet) {
                for d in &rule.declarations {
                    if d.property.starts_with("--") && !names.contains(&d.property) && reads_any(d, &names) {
                        names.push(d.property.clone());
                    }
                }
            }
            if names.len() == before {
                break;
            }
        }
        let mut selectors = Vec::new();
        for rule in all_rules(sheet) {
            let dependent = rule
                .declarations
                .iter()
                .any(|d| (d.property.starts_with("--") && names.contains(&d.property)) || reads_any(d, &names));
            if dependent {
                selectors.extend(rule.selectors.iter());
                if selectors.len() > MAX_SELECTORS {
                    return None;
                }
            }
        }
        Some(Self { names, selectors })
    }

    /// Whether `node`'s style could be affected by the change, by anything but inheritance.
    fn may_reach(&self, doc: &Document, node: NodeId) -> bool {
        if doc.get(node).get_attr("style").is_some_and(|s| s.contains("var(") || s.contains("--")) {
            return true;
        }
        self.selectors.iter().any(|s| matches_complex(s, doc, node))
    }
}

/// The names whose value differs between two maps (added, removed or changed).
fn differing_names(old: &CustomProps, new: &CustomProps) -> Vec<String> {
    let mut names: Vec<String> = old
        .iter()
        .filter(|(k, v)| new.get(k.as_str()) != Some(*v))
        .map(|(k, _)| k.clone())
        .collect();
    names.extend(new.keys().filter(|k| !old.contains_key(k.as_str())).cloned());
    names
}

impl<'s> CustomFlow<'s> {
    /// The flow an element's recomputed style starts, if its change from `old` is in `custom_props`
    /// alone.
    pub(crate) fn start(
        doc: &Document,
        sheet: &'s Stylesheet,
        old: &ComputedStyle,
        new: &ComputedStyle,
    ) -> Option<Self> {
        if disabled() || old.custom_props == new.custom_props || doc.has_author_shadow_roots() {
            return None;
        }
        let mut probe = new.clone();
        probe.custom_props = old.custom_props.clone();
        if probe != *old {
            return None;
        }
        let changed = differing_names(&old.custom_props, &new.custom_props);
        let dep = CustomDep::build(sheet, changed.clone())?;
        Some(Self {
            dep: Rc::new(dep),
            changed: Rc::new(changed),
            old: old.custom_props.clone(),
            new: new.custom_props.clone(),
        })
    }

    /// The style of `node`, a child of the element the flow comes from, when it is the previous
    /// one with the new values of the changed names; `None` when a rule can reach it and it must
    /// go through the cascade.
    pub(crate) fn inherit(&self, doc: &Document, node: NodeId, prev: &ComputedStyle) -> Option<ComputedStyle> {
        if self.dep.may_reach(doc, node) {
            return None;
        }
        let mut style = prev.clone();
        if prev.custom_props.ptr_eq(&self.old) {
            // No declaration of its own: it shares the parent's map and keeps doing so.
            style.custom_props = self.new.clone();
        } else {
            // It declared something of its own, but none of the changed names (a rule that does
            // is dependent, so it would have been reached): those entries are inherited, so they
            // take the parent's new values.
            let map = style.custom_props.make_mut();
            for name in self.changed.iter() {
                match self.new.get(name.as_str()) {
                    Some(value) => {
                        map.insert(name.clone(), value.clone());
                    }
                    None => {
                        map.remove(name.as_str());
                    }
                }
            }
        }
        Some(style)
    }

    /// The flow a child the sheet *could* reach hands on after the cascade gave it `new` in place
    /// of `old`: it continues when the change is in `custom_props` alone and in names the analysis
    /// already covers.
    pub(crate) fn resume(&self, old: &ComputedStyle, new: &ComputedStyle) -> Option<Self> {
        if old.custom_props == new.custom_props {
            return None;
        }
        let mut probe = new.clone();
        probe.custom_props = old.custom_props.clone();
        if probe != *old {
            return None;
        }
        let changed = differing_names(&old.custom_props, &new.custom_props);
        if !changed.iter().all(|n| self.dep.names.contains(n)) {
            return None;
        }
        Some(Self {
            dep: Rc::clone(&self.dep),
            changed: Rc::new(changed),
            old: old.custom_props.clone(),
            new: new.custom_props.clone(),
        })
    }

    /// The flow `node` passes to its own children after [`Self::inherit`] gave it `style`.
    pub(crate) fn below(&self, prev: &ComputedStyle, style: &ComputedStyle) -> Self {
        Self {
            dep: Rc::clone(&self.dep),
            changed: Rc::clone(&self.changed),
            old: prev.custom_props.clone(),
            new: style.custom_props.clone(),
        }
    }
}
