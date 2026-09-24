//! BUG-1112 срез 8: split out of `cascade.rs` (which was already over the
//! 2000-line cap — this cluster must not make it grow further) — whether a
//! rule selector `cascade::compute_style_shareable`'s `RuleIndex` bucketing
//! could hand back as a candidate for two *different* nodes sharing the same
//! [`super::share_cache::ShareKey`] is safe to cache, or must instead
//! contribute a real-match bit to the key (`dynamic_fingerprint`).
//! See [`selector_is_share_safe`]'s doc comment for the induction and
//! [`super::share_cache`]'s module doc for how the two halves compose.

use lumen_core::geom::Size;
use lumen_css_parser::Stylesheet;
use lumen_dom::{Document, NodeId};

use crate::style::{ensure_cascade_index, matches_complex, with_front_cascade_index};

/// THREAD-4 срез 2, BUG-1112: a rule selector this crate's `RuleIndex`
/// bucketing could hand back as a candidate for two *different* nodes that
/// share the same structural key, with a match result that depends on
/// nothing the key does not capture.
///
/// Every ancestor compound (any entry in `tail` before the last) must be
/// built only from `Type`/`Class`/`Id`/`Universal`/`Attribute` parts (the
/// last joined this list in BUG-1112 срез 5 — see `compound_is_share_safe`'s
/// `Attribute` arm for why an ancestor's attributes are pinned just as hard
/// as `Type`/`Class`/`Id` by the `inherited_ptr` induction below) — no
/// pseudo-class, none of which [`build_key`](super::share_cache) pins for an
/// ancestor a combinator reaches into (dynamic state like `:hover`, or
/// sibling-position facts the key has no field for at any depth but the key
/// node's own). The *subject* compound (the last `tail` entry, or `head`
/// when `tail` is empty) allows a few more parts that `build_key` pins
/// directly for the node itself: `Root` (BUG-1112 срез 3),
/// `FirstChild`/`LastChild`/`OnlyChild` (BUG-1112 срез 4, via
/// `is_first_child`/`is_last_child`), and `Where`/`Is`/`Not` (BUG-1112 срез 4,
/// recursively — safe exactly when every selector in their argument list is)
/// — see `compound_is_share_safe` below for exactly which, and
/// `complex_is_share_safe`'s doc comment for how "subject vs ancestor"
/// generalises to "describes the key node vs not" once recursion can cross
/// into a `:where(..)` argument.
///
/// `Descendant`/`Child` combinators ARE allowed (BUG-1112, reversing THREAD-4
/// срез 2's blanket ban): [`super::share_cache`]'s `inherited_ptr` field is
/// the address of the *live* `ComputedStyle` allocation the cascade handed
/// down as this node's `inherited` parameter, and `counters::walk` hands
/// every child of a node the very same allocation (one `Arc` built once,
/// read many times, never rebuilt per child). So whenever two nodes' keys
/// collide on `inherited_ptr`, that pointer equality is not a coincidence
/// of content — it is only reachable by literally being two children of the
/// same live parent, or (by induction) two nodes whose own parents already
/// collided on this cache for the same reason. Either base case forces the
/// *entire* eligible ancestor chain above both nodes to be pairwise
/// tag+attrs-identical (down to `is_svg_presentational_element` for every
/// level involved), all the way to a genuinely shared DOM ancestor — which
/// makes any `Type`/`Class`/`Id`/`Universal`-only ancestor compound match
/// identically for both nodes, at any depth. `NextSibling`/`LaterSibling`
/// stay banned: sibling position is not part of this key at any level, so
/// nothing here proves two colliding nodes even have comparable siblings.
///
/// BUG-1112 срез 8 (ancestor), срез 10 (subject — see
/// [`subject_is_fingerprintable`]): when the abstract check above fails only
/// because of an otherwise-unpinned dynamic part — ANCESTOR compound
/// (dynamic pseudo-class like `:hover`, or a sibling-position pseudo-class
/// the key only pins for the node it was built for) or the SUBJECT compound
/// itself (`node:hover` — a dynamic fact about the key node, not an
/// ancestor) — `sel` is not banned outright any more — it is
/// [`is_fingerprintable`], meaning [`super::share_cache::ShareCache::compute`]
/// (via [`dynamic_fingerprint`]) folds `sel`'s REAL, current
/// [`matches_complex`] result into [`super::share_cache::ShareKey`] as one
/// more bit. `ShareCache` is rebuilt from scratch every pass (see
/// `share_cache.rs`'s module doc — it never survives across a dynamic-state
/// change), so that bit is exactly as fresh as the pass itself; two nodes
/// only collide on a key that includes it when their real, current match
/// result for `sel` agrees, which is the only case sharing is actually safe.
/// Срез 6/7's separate "abstractly unreachable → always safe, no key cost"
/// path is now just the `false` value of the same bit — subsumed, not a
/// special case, so this replaces (not layers alongside) that mechanism.
pub(crate) fn selector_is_share_safe(sel: &lumen_css_parser::ComplexSelector) -> bool {
    complex_is_share_safe(sel, true) || is_fingerprintable(sel)
}

/// BUG-1112 срез 10: union of the two shapes [`dynamic_fingerprint`]'s real
/// match can rescue — [`ancestor_prefix_is_fingerprintable`] (dynamic
/// ANCESTOR compound, subject already structurally safe) or
/// [`subject_is_fingerprintable`] (dynamic SUBJECT compound, every ancestor
/// compound already structurally safe). Both compile down to the exact same
/// action — fold `matches_complex(sel, doc, node)`'s real value into the key
/// — because that call already re-derives the WHOLE selector's match against
/// `node`, so it does not matter on which side of the selector the dynamic
/// part sits; only ONE side may be dynamic (a selector with a dynamic part on
/// BOTH sides, e.g. `.wrap:hover .icon:focus`, is eligible for neither shape
/// and stays unsafe — future work, not a soundness gap in what is fingerprinted
/// today).
fn is_fingerprintable(sel: &lumen_css_parser::ComplexSelector) -> bool {
    ancestor_prefix_is_fingerprintable(sel) || subject_is_fingerprintable(sel)
}

/// BUG-1112 срез 8: one-time-per-pass sheet scan (mirrors `share_cache::
/// sheet_has_position_dependent_subject`) — does `sheet` (or any of its
/// `@layer`/`@media`/`@supports` blocks) contain ANY selector
/// [`dynamic_fingerprint`] would ever need to test? `false` lets
/// [`super::share_cache::ShareCache::compute`] skip
/// `dynamic_fingerprint`'s `RuleIndex::candidates` queries entirely
/// on every eligible node for the (overwhelming majority of) sheets with no
/// fingerprintable rule at all — same cost shape as `track_position`.
pub(crate) fn sheet_has_fingerprintable_selector(sheet: &Stylesheet) -> bool {
    fn rule_has_it(rule: &lumen_css_parser::Rule) -> bool {
        rule.selectors.iter().any(|sel| !complex_is_share_safe(sel, true) && is_fingerprintable(sel))
    }
    fn rules_have_it(rules: &[lumen_css_parser::Rule]) -> bool {
        rules.iter().any(rule_has_it)
    }
    sheet.rules.iter().any(rule_has_it)
        || sheet.layers.iter().any(|l| rules_have_it(&l.rules))
        || sheet.media_rules.iter().any(|m| rules_have_it(&m.rules))
        || sheet.supports_rules.iter().any(|s| rules_have_it(&s.rules))
}

/// BUG-1112 срез 8 — see [`selector_is_share_safe`]'s doc comment. Purely
/// syntactic (no document, no real match): is `sel`'s *shape* eligible for
/// the fingerprint escape hatch at all?
///
/// The subject compound and the combinator kinds are NOT eligible — for
/// reasons unrelated to real document position, so no amount of real
/// matching could rescue them: a dynamic subject part (`node:hover { .. }`)
/// is a per-node fact about `node` itself with nothing ancestor-shaped to
/// fingerprint (the whole point of the escape hatch is a fact about SOME
/// OTHER node in the chain, not the key node itself), and a sibling
/// combinator (`+`/`~`) is banned because [`matches_complex`]'s real match
/// still would not tell us anything reusable — [`super::share_cache::ShareKey`]
/// has no field for sibling position at any depth, and unlike an ancestor's
/// dynamic state, two colliding nodes' *siblings* are not even proven to
/// correspond to each other by the `inherited_ptr` induction, so there is no
/// guarantee the fingerprint vector this produces is even comparing the same
/// selector against comparable document positions. Only a selector whose
/// subject is already abstractly safe and whose every combinator is
/// `Descendant`/`Child` qualifies — exactly the shape srez 6 used to walk
/// with `ancestor_chain_reachable`, but now the shape is all that is needed
/// here: the actual real-vs-permissive matching moves entirely into
/// [`dynamic_fingerprint`], which every fingerprintable selector
/// reaches through the SAME shape check, so the two can never disagree on
/// which selectors qualify.
fn ancestor_prefix_is_fingerprintable(sel: &lumen_css_parser::ComplexSelector) -> bool {
    let mut compounds: Vec<&lumen_css_parser::CompoundSelector> =
        Vec::with_capacity(1 + sel.tail.len());
    let mut combinators: Vec<lumen_css_parser::Combinator> = Vec::with_capacity(sel.tail.len());
    compounds.push(&sel.head);
    for (comb, comp) in &sel.tail {
        combinators.push(*comb);
        compounds.push(comp);
    }
    let n = compounds.len();
    if n == 1 {
        // No ancestor compound at all — nothing for the escape hatch to
        // reach into; the top-level `complex_is_share_safe(sel, true)` call
        // already covers a bare subject compound.
        return false;
    }
    if !compound_is_share_safe(compounds[n - 1], true) {
        // The subject itself is the problem — not eligible, see doc comment.
        return false;
    }
    // A sibling combinator anywhere — not eligible, see doc comment.
    combinators
        .iter()
        .all(|c| matches!(c, lumen_css_parser::Combinator::Descendant | lumen_css_parser::Combinator::Child))
}

/// BUG-1112 срез 10: the mirror image of [`ancestor_prefix_is_fingerprintable`]
/// — is `sel`'s *shape* eligible for the escape hatch when the dynamic part
/// sits on the SUBJECT compound (`node:hover { .. }`, the depth-0 case срез 9
/// found dominant on github.com: `:where(.prc-Link-Link-9ZwDx):where([data-muted=true]):hover`,
/// a single compound with no ancestor at all) instead of an ancestor one?
///
/// Every ancestor compound (everything but the last) must already be
/// structurally safe — `Type`/`Class`/`Id`/`Universal`/`Attribute` only, same
/// `compound_is_share_safe(_, false)` call [`complex_is_share_safe`] uses for
/// an ancestor position — because [`dynamic_fingerprint`]'s real match is
/// only ever recorded per KEY NODE, not per ancestor: if the ancestor part
/// itself were only conditionally equal between two colliding nodes, folding
/// one shared bool for the whole selector would not capture that. What is
/// "wrong" with the subject compound does not matter here (unlike
/// `ancestor_prefix_is_fingerprintable`, this function does not re-check
/// `compound_is_share_safe` on the subject at all) — [`matches_complex`]
/// re-derives the WHOLE selector's real match against `node` regardless of
/// which simple selector inside the subject compound made it dynamic, so a
/// bare `:hover` and something like `.foo:hover:focus` are equally covered by
/// one real-match bit. Combinators are restricted to `Descendant`/`Child` for
/// the same reason as the ancestor case — see
/// [`ancestor_prefix_is_fingerprintable`]'s doc comment.
fn subject_is_fingerprintable(sel: &lumen_css_parser::ComplexSelector) -> bool {
    let mut compounds: Vec<&lumen_css_parser::CompoundSelector> =
        Vec::with_capacity(1 + sel.tail.len());
    let mut combinators: Vec<lumen_css_parser::Combinator> = Vec::with_capacity(sel.tail.len());
    compounds.push(&sel.head);
    for (comb, comp) in &sel.tail {
        combinators.push(*comb);
        compounds.push(comp);
    }
    let n = compounds.len();
    if !compounds[..n - 1].iter().all(|c| compound_is_share_safe(c, false)) {
        // An ancestor compound is itself the problem — not eligible, see
        // doc comment.
        return false;
    }
    // A sibling combinator anywhere — not eligible, see doc comment.
    combinators
        .iter()
        .all(|c| matches!(c, lumen_css_parser::Combinator::Descendant | lumen_css_parser::Combinator::Child))
}

/// BUG-1112 срез 8 (ancestor), срез 10 (subject): the real, current
/// [`matches_complex`] result for every selector [`selector_is_share_safe`]
/// treats as [`is_fingerprintable`] among `node`'s candidates in `sheet` —
/// one `bool` per such selector, in [`crate::rule_index::RuleIndex::candidates`]'s
/// own order.
///
/// Walks the exact same four candidate sources
/// [`crate::style::cascade::compute_style_shareable`] does (top-level rules,
/// `@layer`, `@media`, `@supports`), because a fingerprintable selector
/// living inside any of them is just as real a candidate for `node`. Only
/// the fingerprintable ones cost a [`matches_complex`] call here — every
/// other candidate is already decided by [`selector_is_share_safe`]'s
/// abstract half, for free.
///
/// Deterministic length and order for two nodes with equal `ShareKey`:
/// [`crate::rule_index::RuleIndex::candidates`] is a pure function of
/// `node`'s own tag/id/classes/attrs, which the key already pins
/// byte-for-byte (`share_cache.rs`'s module doc) — so any two nodes this
/// vector could ever be compared against (via `ShareKey`'s `Eq`/`Hash`)
/// are guaranteed to have queried the very same candidate rules in the very
/// same order.
pub(crate) fn dynamic_fingerprint(
    doc: &Document,
    node: NodeId,
    sheet: &Stylesheet,
    viewport: Size,
    dark_mode: bool,
) -> Vec<bool> {
    let node_data = doc.get(node);
    let node_tag = node_data.element_name().map_or("", |q| q.local.as_str());
    let node_id = node_data.get_attr("id");
    let class_attr = node_data.get_attr("class").unwrap_or("");
    let node_classes: Vec<&str> = class_attr.split_whitespace().collect();
    let node_attrs: &[lumen_dom::Attribute] = match &node_data.data {
        lumen_dom::NodeData::Element { attrs, .. } => attrs,
        _ => &[],
    };

    ensure_cascade_index(sheet, viewport, dark_mode);
    let mut sig: Vec<bool> = Vec::new();
    let mut push_from = |rule: &lumen_css_parser::Rule| {
        for sel in &rule.selectors {
            if !complex_is_share_safe(sel, true) && is_fingerprintable(sel) {
                sig.push(matches_complex(sel, doc, node));
            }
        }
    };

    let cands = with_front_cascade_index(|idx| idx.rules.candidates(node_tag, node_id, &node_classes, node_attrs));
    for rule_idx in cands {
        push_from(&sheet.rules[rule_idx]);
    }

    let layer_cands = with_front_cascade_index(|idx| {
        idx.layers
            .candidates(node_tag, node_id, &node_classes, node_attrs)
            .into_iter()
            .filter_map(|flat| idx.layer_rules.get(flat).copied())
            .collect::<Vec<_>>()
    });
    for (block, rule_idx) in layer_cands {
        if let Some(rule) = sheet.layers.get(block).and_then(|l| l.rules.get(rule_idx)) {
            push_from(rule);
        }
    }

    let active_media = with_front_cascade_index(|idx| idx.active_media.clone());
    for (media_i, media) in sheet.media_rules.iter().enumerate() {
        if !active_media[media_i] {
            continue;
        }
        let media_cands = with_front_cascade_index(|idx| {
            idx.media[media_i].candidates(node_tag, node_id, &node_classes, node_attrs)
        });
        for rule_idx in media_cands {
            push_from(&media.rules[rule_idx]);
        }
    }

    let active_supports = with_front_cascade_index(|idx| idx.active_supports.clone());
    for (supports_i, supports) in sheet.supports_rules.iter().enumerate() {
        if !active_supports[supports_i] {
            continue;
        }
        let supports_cands = with_front_cascade_index(|idx| {
            idx.supports[supports_i].candidates(node_tag, node_id, &node_classes, node_attrs)
        });
        for rule_idx in supports_cands {
            push_from(&supports.rules[rule_idx]);
        }
    }

    sig
}

/// `describes_key_node` — does a match of `sel` at this recursion depth speak
/// about the actual node [`ShareKey`](super::share_cache) was built for
/// (`true`), or about some ancestor/relative `sel`'s own subject reaches into
/// (`false`)? The top-level call from [`selector_is_share_safe`] starts
/// `true`: the outer selector's own subject IS the key's node. Recursing into
/// a `:where(..)`/`:is(..)`/`:not(..)` argument list (BUG-1112 срез 4) keeps
/// that same flag — those pseudo-classes match the SAME element the compound
/// they live in belongs to, so if the compound holding them describes the key
/// node, so does every inner selector's own subject; if the compound is
/// itself an ancestor's, so is theirs.
fn complex_is_share_safe(sel: &lumen_css_parser::ComplexSelector, describes_key_node: bool) -> bool {
    // `ComplexSelector::head` is the LEFTMOST (topmost-ancestor) compound and
    // `tail`'s entries run rightward — `matching.rs::matches_complex` matches
    // its `compounds[last]` (the last `tail` entry, or `head` when `tail` is
    // empty) against `node` itself, so that is the one and only compound
    // whose own `describes_key_node` can stay whatever the caller passed in;
    // every earlier compound is an ancestor of it, `describes_key_node=false`
    // regardless of the caller's flag.
    let last_tail_idx = sel.tail.len().checked_sub(1);
    compound_is_share_safe(&sel.head, last_tail_idx.is_none() && describes_key_node)
        && sel.tail.iter().enumerate().all(|(i, (comb, compound))| {
            let is_subject = last_tail_idx == Some(i) && describes_key_node;
            let prev = if i == 0 { &sel.head } else { &sel.tail[i - 1].1 };
            let combinator_ok = matches!(
                comb,
                lumen_css_parser::Combinator::Descendant | lumen_css_parser::Combinator::Child
            ) || (is_subject
                && matches!(comb, lumen_css_parser::Combinator::NextSibling)
                && compound_is_bare_universal(prev));
            combinator_ok && compound_is_share_safe(compound, is_subject)
        })
}

/// BUG-1112 срез 11: `A + B`'s real match, when `A` is a bare `*` (no
/// `Type`/`Class`/`Id`/`Attribute`/pseudo constraining it at all), reduces to
/// "does `B` have ANY immediately preceding element sibling" — which is
/// exactly the negation of [`super::matching::forms::is_first_element_child`],
/// already pinned per-node in [`super::share_cache::ShareKey::is_first_child`]
/// for the SUBJECT (`describes_key_node`) whenever
/// [`super::share_cache::sheet_has_position_dependent_subject`] gates it on.
/// So a `NextSibling` combinator immediately before the subject compound is
/// share-safe too, but ONLY there (not at ancestor position — the key has no
/// per-ancestor sibling field, same reasoning `selector_is_share_safe`'s doc
/// comment gives for banning `NextSibling`/`LaterSibling` everywhere else) and
/// ONLY when the compound to its left is this exact bare shape — anything
/// else (`:not(label) + *`, `h1 + *`, `.foo + *`) needs the SIBLING's own
/// tag/attrs, which no field captures, and stays unsafe.
pub(crate) fn compound_is_bare_universal(compound: &lumen_css_parser::CompoundSelector) -> bool {
    matches!(compound.parts.as_slice(), [lumen_css_parser::SimpleSelector::Universal])
}

fn compound_is_share_safe(compound: &lumen_css_parser::CompoundSelector, describes_key_node: bool) -> bool {
    compound.parts.iter().all(|part| match part {
        lumen_css_parser::SimpleSelector::Type(_)
        | lumen_css_parser::SimpleSelector::Class(_)
        | lumen_css_parser::SimpleSelector::Id(_)
        | lumen_css_parser::SimpleSelector::Universal => true,
        // BUG-1112 срез 3: `:root` matches only the document's root
        // element (always `<html>`), which is never
        // `is_svg_presentational_element` — the caller
        // (`compute_style_shareable`) already gates this whole check on
        // that being true for `node`. So when this compound describes the
        // key node, `:root` is a hard, unconditional non-match for every
        // node this function is ever asked about — as safe as a `Type`
        // mismatch, for any key. Live instrumentation on github.com
        // (BUG-1112 срез 2) found this exact pattern (`:root { --accent: … }`,
        // the common way to declare CSS custom properties) disqualifying
        // every single SVG-presentational node in the document, because
        // the candidate index hands `:root` back for every query (it has
        // no type/class/id to bucket on) — `share_insert` stayed 0 on
        // every real site measured. NOT extended to the ancestor case
        // (`describes_key_node=false`): there, `:root` composed with
        // `Child` would additionally need "is this node's parent literally
        // the root" reasoning the `inherited_ptr` induction does not cover
        // (it only proves identity of the *SVG-presentational* segment of
        // the ancestor chain, not that the chain terminates at `<html>` at
        // any particular depth) — see the module doc's induction.
        lumen_css_parser::SimpleSelector::PseudoClass(lumen_css_parser::PseudoClass::Root) => {
            describes_key_node
        }
        // BUG-1112 срез 3: an attribute selector, when it describes the key
        // node, is a pure function of `node`'s own attribute set — exactly
        // what `ShareKey.attrs` already pins byte-for-byte (see
        // `share_cache.rs`'s module doc: "full attribute set, not just
        // class/id"). Two nodes with an equal key therefore have identical
        // attributes, so `matches_complex` on any operator (`=`, `*=`,
        // `^=`, …) against those attributes is guaranteed to agree between
        // them — same soundness argument as `Class`/`Id` above, just not
        // restricted to those two attribute names. Second blocker live
        // instrumentation found after the `:root` fix above: github.com's
        // dark-mode custom properties
        // (`[data-color-mode=light][data-light-theme*=light] { … }`)
        // disqualified every SVG-presentational node the same way.
        //
        // BUG-1112 срез 5: extended to the ancestor case too — unconditional
        // `true`, not gated on `describes_key_node`. This is not a new
        // soundness argument, it is the SAME `inherited_ptr` induction
        // already trusted for `Type`/`Class`/`Id`/`Universal` at ancestor
        // position (`complex_is_share_safe`'s doc comment above): two nodes
        // whose keys collide have, by that induction, a pairwise
        // **tag+attrs-identical** eligible ancestor chain up to a genuinely
        // shared DOM ancestor — "attrs" there already means the FULL
        // attribute set at every level involved, not just the key node's
        // own, because each ancestor in the chain was itself either (a) the
        // literal same live parent (trivially identical attributes) or (b)
        // a node whose OWN key matched a previous occurrence, which pins
        // *that* node's full `attrs` field the same way srez 3 already
        // established for the subject. An ancestor attribute selector can
        // therefore never discriminate between two colliding-key nodes: if
        // it did, the ancestor's own `attrs` would differ, which would have
        // broken the `inherited_ptr` collision the two nodes rely on in the
        // first place (`build_key` only shares `inherited_ptr` down the
        // literal live `Arc` a node handed its own children, and a
        // cache-hit ancestor's returned style is only reachable through an
        // equal-key, equal-`attrs` earlier node). Regression guard —
        // `an_ancestor_position_attribute_selector_still_disables_sharing`
        // in `style/tests/share_cache.rs` — is retargeted this срез into
        // `an_ancestor_position_attribute_selector_now_allows_sharing_when_
        // the_key_proves_ancestor_identity`, which demonstrates the
        // positive case this induction actually establishes.
        lumen_css_parser::SimpleSelector::Attribute(_) => true,
        // BUG-1112 срез 4: `:first-child`/`:last-child`/`:only-child`, when
        // they describe the key node, are a pure function of `node`'s own
        // sibling position, which `ShareKey.is_first_child`/`is_last_child`
        // (`share_cache.rs::build_key`) pins directly whenever any rule in
        // the sheet needs them — same soundness shape as `Attribute` above
        // (a per-node fact in the key, no ancestor-identity induction
        // needed). Live instrumentation (BUG-1112 срез 3) found
        // `.pagination > :first-child`/`:last-child` and
        // `.btn .octicon:only-child` disqualifying every SVG-presentational
        // node in the document via `RuleIndex`'s `universal` bucket (no
        // type/class/id to index these pseudo-classes on, so they are a
        // "candidate" for every node regardless of whether it is ever a
        // descendant of `.pagination`/`.btn`). Still not extended to the
        // ancestor case: the key has no field for an ancestor's sibling
        // position, only the key node's own.
        lumen_css_parser::SimpleSelector::PseudoClass(
            lumen_css_parser::PseudoClass::FirstChild
            | lumen_css_parser::PseudoClass::LastChild
            | lumen_css_parser::PseudoClass::OnlyChild,
        ) => describes_key_node,
        // BUG-1112 срез 4: `:where(..)`/`:is(..)`/`:not(..)` each match the
        // SAME element the compound they live in belongs to — CSS Selectors
        // L4 §5.4/§17 define all three purely in terms of whether the
        // *argument* selectors match that element (negated, for `:not`;
        // "any of", for `:is`/`:where"; the specificity/polarity difference
        // between them is irrelevant here, only agreement between two
        // colliding-key nodes is). So each is share-safe exactly when every
        // selector in its argument list is, recursed with the SAME
        // `describes_key_node` this compound was given — not
        // unconditionally `true`, because the compound itself may be an
        // ancestor's (see `complex_is_share_safe`'s doc comment). Live
        // instrumentation found this the dominant remaining blocker on
        // github.com: Primer (GitHub's design system) compiles nearly every
        // component class to a `:where(.prc-X-Y-Z)` wrapper for
        // zero-specificity overridability, e.g.
        // `:where(.prc-Link-Link-9ZwDx):where([data-muted=true]):hover` —
        // each `:where()` here wraps exactly one `Class`/`Attribute`
        // selector, already proven safe above; only the trailing `:hover`
        // (correctly, a dynamic state, not a structural fact) keeps such
        // rules unsafe as a whole.
        lumen_css_parser::SimpleSelector::PseudoClass(
            lumen_css_parser::PseudoClass::Where(list)
            | lumen_css_parser::PseudoClass::Is(list)
            | lumen_css_parser::PseudoClass::Not(list),
        ) => list.iter().all(|inner| complex_is_share_safe(inner, describes_key_node)),
        // BUG-1112 срез 4: a `PseudoElement` (`::before`, `::placeholder`,
        // `::-webkit-*`, `::slotted(..)`, …) makes `matches_simple`
        // (`matching.rs:223`) return `false` unconditionally, for ANY node,
        // in the normal element-cascade path this crate's `matches_complex`
        // walks — [`ShareCache::compute`] only ever calls
        // `compute_style_shareable` for real DOM elements (`build_key`
        // requires `NodeData::Element`), never for a synthesized
        // `::before`/`::after` target (`compute_pseudo_element_style` is a
        // separate function this cache never touches), so that hard
        // non-match is not conditional on anything a key could fail to
        // capture — both colliding nodes get `false`, always. Safe
        // regardless of `describes_key_node`, unlike every other exception
        // above. `::slotted(..)` specifically is matched through a
        // dedicated function (`matches_slotted_complex`) entirely outside
        // this crate's normal `rule.selectors`/`matches_complex` walk, but
        // that path only ever runs when `host_shadow.is_some()` — which
        // `compute_style_shareable` already zeroes `shareable` for
        // unconditionally (`own_shadow.is_none() && host_shadow.is_none()
        // && interior_shadow.is_none()`), so there is no interaction to
        // reason about. Live instrumentation (github.com) found this the
        // last blocker after срез 4's `:where`/`:is`/`:not` fix: bare
        // `::placeholder`/`::-webkit-calendar-picker-indicator`/… selectors
        // (no type/class/id — `RuleIndex`'s `universal` bucket) were
        // disqualifying every one of the 391 SVG-presentational candidates
        // in the document.
        lumen_css_parser::SimpleSelector::PseudoElement(_) => true,
        _ => false,
    })
}
