//! BUG-935 срез 64/93 — what separates two versions of the cascade sheet.
//!
//! A page that inserts a `<style>` (CSS-in-JS, an ad widget) or a shell that merges a
//! just-loaded stylesheet into the base one gives the next flush a sheet that differs
//! from the previous flush's by a handful of rules. The incremental flush refused any
//! such sheet (`stylesheet revision changed`) and ran the whole cascade — 0,6–1,3 s on
//! lenta.ru, eight times per page load. [`Stylesheet::changed_style_rules`] names the rules
//! that differ, so the caller can restyle only the elements they select.

use std::collections::HashMap;
use std::fmt::Debug;
use std::hash::{Hash, Hasher};

use super::{Rule, Stylesheet};

impl Stylesheet {
    /// The style rules whose presence or place in the cascade differs between `self` (the
    /// older sheet) and `newer` — added, removed, or moved relative to the others — taken
    /// from the plain rules (`Self::rules`) and from the `@media`/`@supports` blocks (a block
    /// that differs contributes every rule inside it). `None` when the two differ in
    /// anything else (`@layer`, `@property`, `@scope`, `@container`, …): then the effect of
    /// the change is not confined to the elements a rule selects, and the caller has to
    /// recascade it all.
    ///
    /// Why that is enough for an element to keep its style. The cascade orders the rules by
    /// position: first the plain ones, then `@layer`, then the `@media` blocks, then the
    /// `@supports` blocks, each group in source order. Inserting or removing a rule (a block)
    /// shifts no other rule's standing against one of another group, and a block that is equal
    /// in both sheets is active in both (its condition is evaluated per block, against the same
    /// viewport). Within one group, the rules returned here are all that changed: the rest are
    /// present in both sheets in the same relative order (the longest such run is kept). An
    /// element that no returned rule selects therefore matches the same rules in the same
    /// order in both sheets, and cascades to the same style.
    ///
    /// `@font-face` and `@keyframes` do not take part: the cascade reads neither (fonts reach
    /// layout through the shell's registry, keyframes are sampled at animation-tick time from
    /// the live sheet), so a difference in them leaves every computed style as it was.
    ///
    /// The result is empty when the style rules are identical — only provenance or the
    /// CSSOM bookkeeping differed.
    #[must_use]
    pub fn changed_style_rules<'a>(&'a self, newer: &'a Stylesheet) -> Option<Vec<&'a Rule>> {
        if !self.fields_differing_from_style_rules(newer).is_empty() {
            return None;
        }
        let mut changed: Vec<&Rule> = changed_items(&self.rules, &newer.rules);
        for block in changed_items(&self.media_rules, &newer.media_rules) {
            changed.extend(&block.rules);
        }
        for block in changed_items(&self.supports_rules, &newer.supports_rules) {
            changed.extend(&block.rules);
        }
        Some(changed)
    }

    /// Names of the fields the cascade reads that [`Self::changed_style_rules`] cannot
    /// express — what keeps it from answering. Also the reason a live flush logs when it falls
    /// back to the full cascade (BUG-935 срез 93).
    /// `top_level_order` is left out on purpose: it only feeds the CSSOM's `cssRules` view,
    /// the cascade orders by position in the rule vectors.
    #[must_use]
    pub fn fields_differing_from_style_rules(&self, other: &Stylesheet) -> Vec<&'static str> {
        let Stylesheet {
            revision: _,
            rules: _,
            properties,
            media_rules: _,
            imports,
            font_faces: _,
            layer_order,
            layers,
            supports_rules: _,
            keyframes: _,
            counter_styles,
            page_rules,
            scope_rules,
            starting_style_rules,
            view_transition_rules,
            container_rules,
            font_palette_values,
            color_profiles,
            function_rules,
            mixin_rules,
            top_level_order: _,
            top_level_spans: _,
            source: _,
        } = self;
        let mut differing = Vec::new();
        let mut check = |name: &'static str, same: bool| {
            if !same {
                differing.push(name);
            }
        };
        check("properties", *properties == other.properties);
        check("imports", *imports == other.imports);
        check("layer_order", *layer_order == other.layer_order);
        check("layers", *layers == other.layers);
        check("counter_styles", *counter_styles == other.counter_styles);
        check("page_rules", *page_rules == other.page_rules);
        check("scope_rules", *scope_rules == other.scope_rules);
        check("starting_style_rules", *starting_style_rules == other.starting_style_rules);
        check("view_transition_rules", *view_transition_rules == other.view_transition_rules);
        check("container_rules", *container_rules == other.container_rules);
        check("font_palette_values", *font_palette_values == other.font_palette_values);
        check("color_profiles", *color_profiles == other.color_profiles);
        check("function_rules", *function_rules == other.function_rules);
        check("mixin_rules", *mixin_rules == other.mixin_rules);
        differing
    }
}

/// A hash of a rule's content, only to bucket candidates — equality is re-checked on `==`.
fn fingerprint<T: Debug>(item: &T) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    format!("{item:?}").hash(&mut hasher);
    hasher.finish()
}

/// The items of `old` and `new` that were added, removed, or moved relative to the others.
fn changed_items<'a, T: PartialEq + Debug>(old: &'a [T], new: &'a [T]) -> Vec<&'a T> {
    let limit = old.len().min(new.len());
    let prefix = (0..limit).take_while(|&i| old[i] == new[i]).count();
    let suffix = (0..limit - prefix)
        .take_while(|&i| old[old.len() - 1 - i] == new[new.len() - 1 - i])
        .count();
    let old_mid = &old[prefix..old.len() - suffix];
    let new_mid = &new[prefix..new.len() - suffix];
    if old_mid.is_empty() || new_mid.is_empty() {
        // The common case: items appended (or dropped) at one place.
        return old_mid.iter().chain(new_mid).collect();
    }

    // Pair each new item with the first unused equal old one, then keep the largest
    // set of pairs that stays in order (longest increasing subsequence of the old
    // positions): everything outside it is an item that moved, was added, or was removed.
    let mut by_hash: HashMap<u64, Vec<usize>> = HashMap::new();
    for (i, item) in old_mid.iter().enumerate().rev() {
        by_hash.entry(fingerprint(item)).or_default().push(i);
    }
    let mut old_used = vec![false; old_mid.len()];
    let mut pairs: Vec<(usize, usize)> = Vec::new(); // (new index, old index)
    for (j, item) in new_mid.iter().enumerate() {
        let Some(candidates) = by_hash.get_mut(&fingerprint(item)) else { continue };
        if let Some(at) = candidates.iter().rposition(|&i| old_mid[i] == *item) {
            let i = candidates.remove(at);
            old_used[i] = true;
            pairs.push((j, i));
        }
    }
    let kept = longest_increasing_run(&pairs);
    let mut new_kept = vec![false; new_mid.len()];
    for &(j, _) in &kept {
        new_kept[j] = true;
    }
    let mut changed: Vec<&T> = Vec::new();
    changed.extend(old_mid.iter().zip(&old_used).filter(|(_, used)| !**used).map(|(r, _)| r));
    changed.extend(new_mid.iter().zip(&new_kept).filter(|(_, kept)| !**kept).map(|(r, _)| r));
    changed
}

/// The longest run of `pairs` (already ascending in the first component) whose second
/// component ascends too — patience sorting, `O(n log n)`.
pub(super) fn longest_increasing_run(pairs: &[(usize, usize)]) -> Vec<(usize, usize)> {
    // `tails[k]` — index into `pairs` of the smallest tail of an increasing run of length k+1.
    let mut tails: Vec<usize> = Vec::new();
    let mut prev: Vec<Option<usize>> = vec![None; pairs.len()];
    for (at, &(_, old)) in pairs.iter().enumerate() {
        let slot = tails.partition_point(|&t| pairs[t].1 < old);
        prev[at] = slot.checked_sub(1).map(|s| tails[s]);
        if slot == tails.len() {
            tails.push(at);
        } else {
            tails[slot] = at;
        }
    }
    let mut run = Vec::with_capacity(tails.len());
    let mut cursor = tails.last().copied();
    while let Some(at) = cursor {
        run.push(pairs[at]);
        cursor = prev[at];
    }
    run.reverse();
    run
}
