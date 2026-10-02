//! BUG-935 срез 64 — what separates two versions of the cascade sheet.
//!
//! A page that inserts a `<style>` (CSS-in-JS, an ad widget) or a shell that merges a
//! just-loaded stylesheet into the base one gives the next flush a sheet that differs
//! from the previous flush's by a handful of rules. The incremental flush refused any
//! such sheet (`stylesheet revision changed`) and ran the whole cascade — 0,6–1,3 s on
//! lenta.ru, eight times per page load. [`Stylesheet::changed_plain_rules`] names the rules
//! that differ, so the caller can restyle only the elements they select.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use super::{Rule, Stylesheet};

impl Stylesheet {
    /// The plain style rules (`Self::rules`) whose presence or place in the cascade differs
    /// between `self` (the older sheet) and `newer` — added, removed, or moved relative
    /// to the others. `None` when the two differ in anything else (`@media`, `@layer`,
    /// `@font-face`, `@keyframes`, `@property`, …): then the effect of the change is not
    /// confined to the elements a rule selects, and the caller has to recascade it all.
    ///
    /// Why that is enough for an element to keep its style. The cascade orders the plain
    /// rules among themselves by position, and puts them before every `@layer`/`@media`
    /// rule whatever their position — so inserting or removing a plain rule shifts no
    /// other rule's standing against a non-plain one. Among the plain rules, the ones
    /// returned here are all that changed: the rest are present in both sheets in the
    /// same relative order (the longest such run is kept). An element that no returned
    /// rule selects therefore matches the same rules in the same order in both sheets,
    /// and cascades to the same style.
    ///
    /// The result is empty when the plain rules are identical — only provenance or the
    /// CSSOM bookkeeping differed.
    #[must_use]
    pub fn changed_plain_rules<'a>(&'a self, newer: &'a Stylesheet) -> Option<Vec<&'a Rule>> {
        if !self.same_apart_from_plain_rules(newer) {
            return None;
        }
        let (old, new) = (&self.rules, &newer.rules);
        let limit = old.len().min(new.len());
        let prefix = (0..limit).take_while(|&i| old[i] == new[i]).count();
        let suffix = (0..limit - prefix)
            .take_while(|&i| old[old.len() - 1 - i] == new[new.len() - 1 - i])
            .count();
        let old_mid = &old[prefix..old.len() - suffix];
        let new_mid = &new[prefix..new.len() - suffix];
        if old_mid.is_empty() || new_mid.is_empty() {
            // The common case: rules appended (or dropped) at one place.
            return Some(old_mid.iter().chain(new_mid).collect());
        }

        // Pair each new rule with the first unused equal old one, then keep the largest
        // set of pairs that stays in order (longest increasing subsequence of the old
        // positions): everything outside it is a rule that moved, was added, or was removed.
        let mut by_hash: HashMap<u64, Vec<usize>> = HashMap::new();
        for (i, rule) in old_mid.iter().enumerate().rev() {
            by_hash.entry(fingerprint(rule)).or_default().push(i);
        }
        let mut old_used = vec![false; old_mid.len()];
        let mut pairs: Vec<(usize, usize)> = Vec::new(); // (new index, old index)
        for (j, rule) in new_mid.iter().enumerate() {
            let Some(candidates) = by_hash.get_mut(&fingerprint(rule)) else { continue };
            if let Some(at) = candidates.iter().rposition(|&i| old_mid[i] == *rule) {
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
        let mut changed: Vec<&Rule> = Vec::new();
        changed.extend(old_mid.iter().zip(&old_used).filter(|(_, used)| !**used).map(|(r, _)| r));
        changed.extend(new_mid.iter().zip(&new_kept).filter(|(_, kept)| !**kept).map(|(r, _)| r));
        Some(changed)
    }

    /// Every field the cascade reads other than [`Self::rules`] compares equal.
    /// `top_level_order` is left out on purpose: it only feeds the CSSOM's `cssRules` view,
    /// the cascade orders by position in the rule vectors.
    fn same_apart_from_plain_rules(&self, other: &Stylesheet) -> bool {
        let Stylesheet {
            revision: _,
            rules: _,
            properties,
            media_rules,
            imports,
            font_faces,
            layer_order,
            layers,
            supports_rules,
            keyframes,
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
        *properties == other.properties
            && *media_rules == other.media_rules
            && *imports == other.imports
            && *font_faces == other.font_faces
            && *layer_order == other.layer_order
            && *layers == other.layers
            && *supports_rules == other.supports_rules
            && *keyframes == other.keyframes
            && *counter_styles == other.counter_styles
            && *page_rules == other.page_rules
            && *scope_rules == other.scope_rules
            && *starting_style_rules == other.starting_style_rules
            && *view_transition_rules == other.view_transition_rules
            && *container_rules == other.container_rules
            && *font_palette_values == other.font_palette_values
            && *color_profiles == other.color_profiles
            && *function_rules == other.function_rules
            && *mixin_rules == other.mixin_rules
    }
}

/// A hash of a rule's content, only to bucket candidates — equality is re-checked on `==`.
fn fingerprint(rule: &Rule) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    format!("{rule:?}").hash(&mut hasher);
    hasher.finish()
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
